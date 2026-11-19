// The lens glass: what is behind the open notch, bent by a soft round bevel at its edges,
// blurred, saturated and lit along its rim.
Texture2D behind : register(t0);
SamplerState linear_clamp : register(s0);

cbuffer Params : register(b0) {
    float2 source_size;  // the screen read, px
    float2 origin;       // the window's top-left inside it, px
    float4 outline;      // x, y, width, height in window px
    float radius;
    float scale;
    float bevel;         // how far in the bevel curves, logical px
    float strength;      // how far the very edge bends, logical px
    float blur;          // the blur's standard deviation, px
    float dispersion;    // colour split, as a share of the bend
    float2 unused;
};

struct Vertex { float4 pos : SV_Position; };

Vertex vs(uint id : SV_VertexID) {
    Vertex o;
    float2 uv = float2((id << 1) & 2, id & 2);
    o.pos = float4(uv * float2(2, -2) + float2(-1, 1), 0, 1);
    return o;
}

float shape(float2 p) {
    float2 half_size = outline.zw * 0.5;
    float2 q = abs(p - outline.xy - half_size) - half_size + radius;
    return length(max(q, 0)) + min(max(q.x, q.y), 0) - radius;
}

// A Gaussian of `blur`, as taps on a golden-angle spiral out to two deviations, each read from
// a mip level whose texels are about half a deviation: a grid of taps would line up with fine
// patterns behind and keep them.
static const int TAPS = 16;

float3 blurred(float2 px) {
    float lod = max(log2(blur * 0.5), 0);
    float3 sum = 0;
    float total = 0;
    [unroll] for (int i = 0; i < TAPS; i++) {
        float r = 2 * blur * sqrt((i + 0.5) / TAPS);
        float a = i * 2.39996323;
        float2 at = clamp(px + r * float2(cos(a), sin(a)), 0.5, source_size - 0.5);
        float weight = exp(-0.5 * (r * r) / (blur * blur));
        sum += behind.SampleLevel(linear_clamp, at / source_size, lod).rgb * weight;
        total += weight;
    }
    return sum / total;
}

float4 ps(Vertex input) : SV_Target {
    float2 p = input.pos.xy;
    float d = shape(p);
    float alpha = saturate(0.5 - d);
    if (alpha <= 0) return 0;

    float2 normal = normalize(float2(
        shape(p + float2(1, 0)) - shape(p - float2(1, 0)),
        shape(p + float2(0, 1)) - shape(p - float2(0, 1))) + 1e-5);

    // 1 at the edge, 0 where the bevel meets the flat face. A soft round bevel bends gently
    // from far in and most at the edge.
    float rim = 1 - saturate(-d / (bevel * scale));
    float bend = rim * rim;
    float2 displacement = normal * bend * strength * scale;

    float2 q = p + origin;
    float3 color;
    color.r = blurred(q + displacement * (1 + dispersion)).r;
    color.g = blurred(q + displacement).g;
    color.b = blurred(q + displacement * (1 - dispersion)).b;

    float luma = dot(color, float3(0.2126, 0.7152, 0.0722));
    color = lerp(luma, color, 1.35);

    // Light from above left: a soft bright rim, brighter where it faces the light, and a
    // darker inner rim on the far side for thickness.
    float2 light = normalize(float2(-0.55, -0.85));
    float facing = saturate(dot(normal, light) * 0.5 + 0.5);
    color += pow(rim, 6) * (0.10 + 0.35 * facing * facing);
    color *= 1 - 0.15 * pow(rim, 3) * (1 - facing);

    return float4(color * alpha, alpha);
}
