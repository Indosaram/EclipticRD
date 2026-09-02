import Foundation
import MetalKit
import CoreVideo

public class MetalRenderer: NSObject, MTKViewDelegate {
    public weak var mtkView: MTKView?
    private var device: MTLDevice?
    private var commandQueue: MTLCommandQueue?
    private var pipelineState: MTLRenderPipelineState?
    private var cursorPipelineState: MTLRenderPipelineState?
    private var textureCache: CVMetalTextureCache?
    private var currentTexture: MTLTexture?
    private var cursorTexture: MTLTexture?
    private var vertexBuffer: MTLBuffer?
    private let textureLock = NSLock()
    private let cursorLock = NSLock()
    private var cursorX: Float = 0
    private var cursorY: Float = 0

    private struct CursorUniforms {
        var position: SIMD2<Float>
        var size: SIMD2<Float>
    }

    public override init() { super.init() }

    public init?(mtkView: MTKView) {
        super.init()
        guard let device = mtkView.device ?? MTLCreateSystemDefaultDevice() else { return nil }
        self.device = device
        self.mtkView = mtkView
        mtkView.device = device
        mtkView.delegate = self
        mtkView.framebufferOnly = false
        mtkView.colorPixelFormat = .bgra8Unorm

        commandQueue = device.makeCommandQueue()

        // Create texture cache
        var cache: CVMetalTextureCache?
        CVMetalTextureCacheCreate(kCFAllocatorDefault, nil, device, nil, &cache)
        textureCache = cache

        // Create vertex buffer (full-screen quad for video rendering)
        let vertices: [Float] = [
            -1, -1, 0, 1,   // bottom-left  (pos.xy, tex.xy)
             1, -1, 1, 1,   // bottom-right
            -1,  1, 0, 0,   // top-left
             1,  1, 1, 0,   // top-right
        ]
        vertexBuffer = device.makeBuffer(bytes: vertices, length: vertices.count * MemoryLayout<Float>.size, options: .storageModeShared)

        // Generate programmatic mouse cursor texture
        setupCursorTexture()

        // Create pipelines
        setupPipeline()
    }

    private func setupCursorTexture() {
        guard let device = device else { return }
        
        let width = 32
        let height = 32
        var pixels = [UInt32](repeating: 0, count: width * height)
        
        // Classic Mac pointer arrow pattern: 'B' = black border, 'W' = white fill, '.' = transparent
        let pattern: [String] = [
            "B",
            "BB",
            "BWB",
            "BWWB",
            "BWWWB",
            "BWWWWB",
            "BWWWWWB",
            "BWWWWWWB",
            "BWWWWWWWB",
            "BWWWWWWWWB",
            "BWWWWWWWWWB",
            "BWWWWWWWWWWB",
            "BWWWWWWWWWWWB",
            "BWWWWWWWWWWWWB",
            "BWWWWWWWWWWWWWB",
            "BWWWWWWBBBBBBBBB",
            "BWWWWWB",
            "BWWBWWB",
            "BWB.BWB",
            "B...BWB",
            "....BWB",
            ".....B"
        ]
        
        for y in 0..<height {
            for x in 0..<width {
                var color: UInt32 = 0x00000000 // transparent
                if y < pattern.count {
                    let row = pattern[y]
                    if x < row.count {
                        let char = row[row.index(row.startIndex, offsetBy: x)]
                        if char == "B" {
                            color = 0xFF000000 // Black border (alpha 0xFF, BGRA order)
                        } else if char == "W" {
                            color = 0xFFFFFFFF // White fill (alpha 0xFF, BGRA order)
                        }
                    }
                }
                pixels[y * width + x] = color
            }
        }
        
        let descriptor = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .bgra8Unorm, width: width, height: height, mipmapped: false)
        descriptor.usage = .shaderRead
        cursorTexture = device.makeTexture(descriptor: descriptor)
        
        let region = MTLRegionMake2D(0, 0, width, height)
        cursorTexture?.replace(region: region, mipmapLevel: 0, withBytes: pixels, bytesPerRow: width * 4)
    }

    private func setupPipeline() {
        guard let device = device else { return }

        // Shader source embedding as a CLI fallback in case the Default Library isn't bundled (e.g. TestCLI runner)
        let shaderSource = """
        #include <metal_stdlib>
        using namespace metal;
        struct VertexOut {
            float4 position [[position]];
            float2 texCoord;
        };
        vertex VertexOut videoVertex(uint vid [[vertex_id]], constant float4 *vertices [[buffer(0)]]) {
            VertexOut out;
            out.position = float4(vertices[vid].xy, 0, 1);
            out.texCoord = vertices[vid].zw;
            return out;
        }
        fragment half4 videoFragment(VertexOut in [[stage_in]], texture2d<half> tex [[texture(0)]]) {
            constexpr sampler s(mag_filter::linear, min_filter::linear);
            return tex.sample(s, in.texCoord);
        }
        struct CursorUniforms {
            float2 position;
            float2 size;
        };
        vertex VertexOut cursorVertex(uint vid [[vertex_id]], constant CursorUniforms &uniforms [[buffer(0)]]) {
            float2 ndcPos = float2(uniforms.position.x * 2.0 - 1.0, (1.0 - uniforms.position.y) * 2.0 - 1.0);
            float2 ndcSize = uniforms.size;
            float2 positions[4] = {
                float2(ndcPos.x, ndcPos.y),
                float2(ndcPos.x + ndcSize.x, ndcPos.y),
                float2(ndcPos.x, ndcPos.y - ndcSize.y),
                float2(ndcPos.x + ndcSize.x, ndcPos.y - ndcSize.y)
            };
            float2 texCoords[4] = {
                float2(0.0, 0.0),
                float2(1.0, 0.0),
                float2(0.0, 1.0),
                float2(1.0, 1.0)
            };
            VertexOut out;
            out.position = float4(positions[vid], 0, 1);
            out.texCoord = texCoords[vid];
            return out;
        }
        fragment half4 cursorFragment(VertexOut in [[stage_in]], texture2d<half> tex [[texture(0)]]) {
            constexpr sampler s(mag_filter::linear, min_filter::linear);
            half4 color = tex.sample(s, in.texCoord);
            if (color.a < 0.05) { discard_fragment(); }
            return color;
        }
        """

        do {
            // 1. Try to load precompiled shaders from bundle's default library (Precompilation)
            // 2. Fallback to CLI JIT compilation to guarantee TestCLI and E2ETests continue passing
            let library: MTLLibrary
            if let defaultLibrary = device.makeDefaultLibrary() {
                library = defaultLibrary
                ERDLog.info("[MetalRenderer] Loaded precompiled shaders from .metallib!")
            } else {
                library = try device.makeLibrary(source: shaderSource, options: nil)
                ERDLog.info("[MetalRenderer] Falling back to dynamic shader JIT compilation")
            }

            // Create Video Pipeline State
            let videoDescriptor = MTLRenderPipelineDescriptor()
            videoDescriptor.vertexFunction = library.makeFunction(name: "videoVertex")
            videoDescriptor.fragmentFunction = library.makeFunction(name: "videoFragment")
            videoDescriptor.colorAttachments[0].pixelFormat = .bgra8Unorm
            pipelineState = try device.makeRenderPipelineState(descriptor: videoDescriptor)

            // Create Cursor Pipeline State (with Alpha Blending enabled)
            let cursorDescriptor = MTLRenderPipelineDescriptor()
            cursorDescriptor.vertexFunction = library.makeFunction(name: "cursorVertex")
            cursorDescriptor.fragmentFunction = library.makeFunction(name: "cursorFragment")
            cursorDescriptor.colorAttachments[0].pixelFormat = .bgra8Unorm
            cursorDescriptor.colorAttachments[0].isBlendingEnabled = true
            cursorDescriptor.colorAttachments[0].rgbBlendOperation = .add
            cursorDescriptor.colorAttachments[0].alphaBlendOperation = .add
            cursorDescriptor.colorAttachments[0].sourceRGBBlendFactor = .sourceAlpha
            cursorDescriptor.colorAttachments[0].sourceAlphaBlendFactor = .sourceAlpha
            cursorDescriptor.colorAttachments[0].destinationRGBBlendFactor = .oneMinusSourceAlpha
            cursorDescriptor.colorAttachments[0].destinationAlphaBlendFactor = .oneMinusSourceAlpha
            cursorPipelineState = try device.makeRenderPipelineState(descriptor: cursorDescriptor)

        } catch {
            ERDLog.error("[MetalRenderer] Pipeline creation error: \(error)")
        }
    }

    public func displayFrame(_ pixelBuffer: CVPixelBuffer) {
        guard let cache = textureCache, let _ = device else { return }

        let width = CVPixelBufferGetWidth(pixelBuffer)
        let height = CVPixelBufferGetHeight(pixelBuffer)

        var cvTexture: CVMetalTexture?
        CVMetalTextureCacheCreateTextureFromImage(kCFAllocatorDefault, cache, pixelBuffer, nil, .bgra8Unorm, width, height, 0, &cvTexture)

        guard let cvTex = cvTexture, let texture = CVMetalTextureGetTexture(cvTex) else { return }

        textureLock.lock()
        currentTexture = texture
        textureLock.unlock()
    }

    public func updateCursor(x: Float, y: Float) {
        cursorLock.lock()
        cursorX = x
        cursorY = y
        cursorLock.unlock()
    }

    // MARK: - MTKViewDelegate

    public func draw(in view: MTKView) {
        textureLock.lock()
        let texture = currentTexture
        textureLock.unlock()

        guard let texture = texture,
              let pipeline = pipelineState,
              let commandBuffer = commandQueue?.makeCommandBuffer(),
              let renderPassDescriptor = view.currentRenderPassDescriptor,
              let encoder = commandBuffer.makeRenderCommandEncoder(descriptor: renderPassDescriptor) else {
            return
        }

        // Draw video frame
        encoder.setRenderPipelineState(pipeline)
        encoder.setVertexBuffer(vertexBuffer, offset: 0, index: 0)
        encoder.setFragmentTexture(texture, index: 0)
        encoder.drawPrimitives(type: .triangleStrip, vertexStart: 0, vertexCount: 4)

        // Draw mouse cursor overlay
        cursorLock.lock()
        let cx = cursorX
        let cy = cursorY
        cursorLock.unlock()

        if let cTexture = cursorTexture, let cPipeline = cursorPipelineState, cx >= 0.0 && cx <= 1.0 && cy >= 0.0 && cy <= 1.0 {
            let viewWidth = Float(view.drawableSize.width)
            let viewHeight = Float(view.drawableSize.height)
            
            if viewWidth > 0 && viewHeight > 0 {
                // Keep the cursor size constant (exactly 32x32 screen pixels) regardless of resolution
                let ndcWidth = (32.0 / viewWidth) * 2.0
                let ndcHeight = (32.0 / viewHeight) * 2.0
                
                var uniforms = CursorUniforms(
                    position: SIMD2<Float>(cx, cy),
                    size: SIMD2<Float>(ndcWidth, ndcHeight)
                )
                
                encoder.setRenderPipelineState(cPipeline)
                encoder.setVertexBytes(&uniforms, length: MemoryLayout<CursorUniforms>.size, index: 0)
                encoder.setFragmentTexture(cTexture, index: 0)
                encoder.drawPrimitives(type: .triangleStrip, vertexStart: 0, vertexCount: 4)
            }
        }

        encoder.endEncoding()

        if let drawable = view.currentDrawable {
            commandBuffer.present(drawable)
        }
        commandBuffer.commit()
    }

    public func mtkView(_ view: MTKView, drawableSizeWillChange size: CGSize) {}
}
