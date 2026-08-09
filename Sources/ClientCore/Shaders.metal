#include <metal_stdlib>
using namespace metal;

struct VertexOut {
    float4 position [[position]];
    float2 texCoord;
};

// 1. Video Shaders
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

// 2. Cursor Shaders
struct CursorUniforms {
    float2 position; // normalized cursor position (0..1, where 0,0 is top-left)
    float2 size;     // cursor size in NDC (Normalized Device Coordinates)
};

vertex VertexOut cursorVertex(uint vid [[vertex_id]], constant CursorUniforms &uniforms [[buffer(0)]]) {
    // Convert normalized coordinates (0..1, top-left origin) to NDC (-1..1, bottom-left origin)
    float2 ndcPos = float2(uniforms.position.x * 2.0 - 1.0, (1.0 - uniforms.position.y) * 2.0 - 1.0);
    float2 ndcSize = uniforms.size;
    
    // Quad vertices: top-left, top-right, bottom-left, bottom-right
    float2 positions[4] = {
        float2(ndcPos.x, ndcPos.y),                     // top-left
        float2(ndcPos.x + ndcSize.x, ndcPos.y),           // top-right
        float2(ndcPos.x, ndcPos.y - ndcSize.y),           // bottom-left
        float2(ndcPos.x + ndcSize.x, ndcPos.y - ndcSize.y)  // bottom-right
    };
    
    float2 texCoords[4] = {
        float2(0.0, 0.0), // top-left
        float2(1.0, 0.0), // top-right
        float2(0.0, 1.0), // bottom-left
        float2(1.0, 1.0)  // bottom-right
    };
    
    VertexOut out;
    out.position = float4(positions[vid], 0, 1);
    out.texCoord = texCoords[vid];
    return out;
}

fragment half4 cursorFragment(VertexOut in [[stage_in]], texture2d<half> tex [[texture(0)]]) {
    constexpr sampler s(mag_filter::linear, min_filter::linear);
    half4 color = tex.sample(s, in.texCoord);
    
    // Discard transparent pixels to enable smooth alpha cutouts of the cursor shape
    if (color.a < 0.05) {
        discard_fragment();
    }
    
    return color;
}
