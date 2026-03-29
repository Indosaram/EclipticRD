import Foundation
import MetalKit
import CoreVideo

public class MetalRenderer: NSObject, MTKViewDelegate {
    public var mtkView: MTKView?
    private var device: MTLDevice?
    private var commandQueue: MTLCommandQueue?
    private var pipelineState: MTLRenderPipelineState?
    private var textureCache: CVMetalTextureCache?
    private var currentTexture: MTLTexture?
    private var vertexBuffer: MTLBuffer?
    private let textureLock = NSLock()
    private let cursorLock = NSLock()
    private var cursorX: Float = 0
    private var cursorY: Float = 0

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

        // Create vertex buffer (full-screen quad)
        let vertices: [Float] = [
            -1, -1, 0, 1,   // bottom-left  (pos.xy, tex.xy)
             1, -1, 1, 1,   // bottom-right
            -1,  1, 0, 0,   // top-left
             1,  1, 1, 0,   // top-right
        ]
        vertexBuffer = device.makeBuffer(bytes: vertices, length: vertices.count * MemoryLayout<Float>.size, options: .storageModeShared)

        // Create pipeline
        setupPipeline()
    }

    private func setupPipeline() {
        guard let device = device else { return }

        // TODO: For production, pre-compile this shader into a .metallib and bundle it
        // to avoid runtime compilation overhead. Current inline shader is kept for simplicity
        // since it's trivial (passthrough texture sampling).
        let shaderSource = """
        #include <metal_stdlib>
        using namespace metal;
        struct VertexOut {
            float4 position [[position]];
            float2 texCoord;
        };
        vertex VertexOut vertexShader(uint vid [[vertex_id]], constant float4 *vertices [[buffer(0)]]) {
            VertexOut out;
            out.position = float4(vertices[vid].xy, 0, 1);
            out.texCoord = vertices[vid].zw;
            return out;
        }
        fragment half4 fragmentShader(VertexOut in [[stage_in]], texture2d<half> tex [[texture(0)]]) {
            constexpr sampler s(mag_filter::linear, min_filter::linear);
            return tex.sample(s, in.texCoord);
        }
        """

        do {
            let library = try device.makeLibrary(source: shaderSource, options: nil)
            let descriptor = MTLRenderPipelineDescriptor()
            descriptor.vertexFunction = library.makeFunction(name: "vertexShader")
            descriptor.fragmentFunction = library.makeFunction(name: "fragmentShader")
            descriptor.colorAttachments[0].pixelFormat = .bgra8Unorm
            pipelineState = try device.makeRenderPipelineState(descriptor: descriptor)
        } catch {
            ERDLog.error("[MetalRenderer] Pipeline error: \(error)")
        }
    }

    public func displayFrame(_ pixelBuffer: CVPixelBuffer) {
        guard let cache = textureCache, let device = device else { return }

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
              let encoder = commandBuffer.makeRenderCommandEncoder(descriptor: renderPassDescriptor) else { return }

        encoder.setRenderPipelineState(pipeline)
        encoder.setVertexBuffer(vertexBuffer, offset: 0, index: 0)
        encoder.setFragmentTexture(texture, index: 0)
        encoder.drawPrimitives(type: .triangleStrip, vertexStart: 0, vertexCount: 4)
        encoder.endEncoding()

        if let drawable = view.currentDrawable {
            commandBuffer.present(drawable)
        }
        commandBuffer.commit()
    }

    public func mtkView(_ view: MTKView, drawableSizeWillChange size: CGSize) {}
}
