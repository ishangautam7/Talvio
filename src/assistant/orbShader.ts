/**
 * The orb's picture: one fragment shader that draws a sphere of light — slow
 * ribbons of colour drifting through a glassy body, a bright rim, and a soft
 * halo — on a transparent canvas.
 *
 * Colour is Talvio's (docs/talvio-brand.md §5): the icon's gradient (#FF8F3F →
 * #EE3A5C) and the coral accent rise from the lit side, the ink tile's
 * blue-violet (#262B6B → #11122E) fills the depth, and warm paper light
 * (#FFE7D9) is where the ribbons cross. It is drawn as light, not as a lit
 * ball: no hard specular, no texture, nothing that reads as a material.
 *
 * The output is premultiplied, so the halo composites as light over whatever
 * is behind the window, never as a dark or milky band.
 *
 * GLSL ES 1.0 on purpose: it is what every WebView's WebGL 1 accepts.
 */

export const ORB_VERTEX = `
attribute vec2 a_pos;
varying vec2 v_uv;
void main() {
  v_uv = a_pos;
  gl_Position = vec4(a_pos, 0.0, 1.0);
}
`;

export const ORB_FRAGMENT = `
precision highp float;
varying vec2 v_uv;
uniform float u_px;      // one device pixel in uv units
uniform float u_time;    // seconds, for periodic motion
uniform float u_flow;    // accumulated flow distance (speed integrated in JS)
uniform float u_level;   // smoothed voice level, 0..1
uniform float u_energy;  // overall light, 0..1
uniform float u_listen;
uniform float u_think;
uniform float u_speak;
uniform float u_mute;
uniform float u_error;
uniform float u_wobble;  // 0 with reduced motion

float hash(vec3 p) {
  p = fract(p * 0.3183099 + 0.1);
  p *= 17.0;
  return fract(p.x * p.y * p.z * (p.x + p.y + p.z));
}

float noise(vec3 x) {
  vec3 i = floor(x);
  vec3 f = fract(x);
  f = f * f * (3.0 - 2.0 * f);
  return mix(
    mix(mix(hash(i + vec3(0.0, 0.0, 0.0)), hash(i + vec3(1.0, 0.0, 0.0)), f.x),
        mix(hash(i + vec3(0.0, 1.0, 0.0)), hash(i + vec3(1.0, 1.0, 0.0)), f.x), f.y),
    mix(mix(hash(i + vec3(0.0, 0.0, 1.0)), hash(i + vec3(1.0, 0.0, 1.0)), f.x),
        mix(hash(i + vec3(0.0, 1.0, 1.0)), hash(i + vec3(1.0, 1.0, 1.0)), f.x), f.y),
    f.z);
}

float fbm(vec3 p) {
  float v = 0.0;
  float a = 0.55;
  for (int i = 0; i < 3; i++) {
    v += a * noise(p);
    p = p * 1.9 + vec3(1.7, 9.2, 3.1);
    a *= 0.5;
  }
  return v;
}

mat2 rot(float a) {
  float c = cos(a);
  float s = sin(a);
  return mat2(c, -s, s, c);
}

// A soft ribbon of light along a wave: brightest on the wave, falling off
// smoothly to either side.
float ribbon(vec2 w, float offset, float freq, float amp, float phase, float width) {
  float y = offset + amp * sin(w.x * freq + phase) + amp * 0.35 * sin(w.x * freq * 2.3 - phase * 1.4);
  float d = (w.y - y) / width;
  // A bright core inside a wide soft bloom: light, not paint.
  return 0.55 * exp(-d * d * 4.0) + 0.5 * exp(-d * d * 0.6);
}

void main() {
  vec2 p = v_uv;
  float r = length(p);
  float ang = atan(p.y, p.x);
  vec2 dir = vec2(cos(ang), sin(ang));
  float voice = (u_listen + u_speak) * u_level;

  // The sphere: half the canvas, swelling with a voice and breathing while it
  // thinks. The rest of the canvas is room for the halo.
  float radius = 0.5 * (1.0 + 0.09 * voice + 0.02 * u_think * sin(u_time * 1.7) * u_wobble);

  // A living edge: a slow, soft wave around the rim that a voice stirs up.
  float rimN = noise(vec3(dir * 1.4, u_flow * 1.3)) - 0.5;
  float rimN2 = noise(vec3(dir * 2.7 + 7.0, u_flow * 2.1)) - 0.5;
  float edgeAmp = (0.008 + 0.06 * voice + 0.006 * u_think) * u_wobble;
  float edge = radius * (1.0 + edgeAmp * (rimN * 1.5 + rimN2 * 0.7));
  float d = r - edge;
  float aa = 1.5 * u_px;
  float inside = 1.0 - smoothstep(-aa, aa, d);

  // Position on the sphere: the unit disc, and how far toward the viewer.
  vec2 q = p / edge;
  float z = sqrt(max(0.0, 1.0 - dot(q, q)));

  // The body's coordinates, drifting: a gentle warp so the ribbons bend like
  // light in liquid, and a slow turn of the whole (quicker while thinking).
  // The ribbons lie roughly level, like a voice's waveform, rocking a little;
  // only while thinking does the whole body turn, slowly.
  float turn = 0.32 * sin(u_flow * 0.21) - 0.18 + u_think * u_time * 0.4 * u_wobble;
  vec2 w = rot(turn) * q;
  // Lensing: the body looks deeper near the rim, like a glass sphere.
  w *= 1.0 / (0.62 + 0.38 * z);
  vec2 warp = vec2(
    fbm(vec3(w * 0.9, u_flow * 0.5)),
    fbm(vec3(w * 0.9 + 4.3, u_flow * 0.5 + 2.0))
  ) - 0.45;
  w += warp * (0.55 + 0.5 * voice);

  vec3 ink = vec3(0.067, 0.071, 0.180);
  vec3 indigo = vec3(0.149, 0.169, 0.420);
  vec3 violet = vec3(0.40, 0.27, 0.70);
  vec3 rose = vec3(0.933, 0.227, 0.361);
  vec3 coral = vec3(1.0, 0.416, 0.322);
  vec3 orange = vec3(1.0, 0.561, 0.247);
  vec3 paper = vec3(1.0, 0.906, 0.851);

  // The body: deep blue-violet, warming toward the lit side.
  float lit = clamp(0.5 + 0.55 * dot(q, normalize(vec2(-0.5, 0.86))), 0.0, 1.0);
  // The icon's own diagonal: coral light at the top left, through rose and
  // violet, into the ink tile's indigo at the bottom right.
  vec3 col = mix(ink, indigo, smoothstep(0.0, 0.42, lit));
  col = mix(col, mix(indigo, violet, 0.6), smoothstep(0.3, 0.68, lit) * 0.8);
  col = mix(col, mix(rose, violet, 0.35) * 0.85, smoothstep(0.55, 0.9, lit) * 0.6);
  col = mix(col, mix(coral, orange, 0.25) * 0.9, smoothstep(0.82, 1.08, lit) * 0.55);

  // Ribbons of light drifting through it. A voice lifts their swing and
  // their brightness; thinking cools them a little toward violet.
  float swing = 0.16 + 0.18 * voice + 0.04 * u_think;
  float ph = u_flow * 2.2;
  float r1 = ribbon(w, 0.18, 2.1, swing, ph, 0.22 + 0.08 * voice);
  float r2 = ribbon(w, -0.12, 1.6, swing * 1.2, -ph * 0.8 + 1.7, 0.26);
  float r3 = ribbon(w, 0.42, 2.8, swing * 0.8, ph * 1.3 + 3.1, 0.16);
  float glow = 0.55 + 0.75 * u_energy;
  vec3 warm1 = mix(coral, orange, 0.45);
  vec3 warm2 = mix(rose, coral, 0.55);
  vec3 cool = mix(violet, rose, 0.3);
  // Ribbons take the body over where they pass (so coral stays coral rather
  // than turning pink on violet), plus a little added light for the glow.
  float k1 = clamp(r1 * (0.7 + 0.3 * u_energy), 0.0, 1.0);
  float k2 = clamp(r2 * (0.55 + 0.3 * u_energy), 0.0, 1.0);
  float k3 = clamp(r3 * (0.45 + 0.3 * u_energy), 0.0, 1.0);
  col = mix(col, mix(warm2, cool, 0.25 + 0.45 * u_think), k2);
  col = mix(col, warm1, k1);
  col = mix(col, mix(orange, paper, 0.45), k3);
  col += (r1 * warm1 + r2 * warm2 * 0.6 + r3 * orange * 0.5) * 0.22 * glow;
  // Where ribbons cross, the light runs to paper white.
  float cross_ = r1 * r2 + r1 * r3 * 0.8;
  col = mix(col, paper, clamp(cross_ * (0.55 + 0.45 * u_energy), 0.0, 0.85));

  // Depth: the middle glows from within, the far edge sinks.
  col *= 0.7 + 0.42 * z;
  col += warm1 * 0.18 * exp(-dot(q, q) * 2.6) * (0.35 + u_energy);

  // Glass: a broad soft sheen on the upper left, and a bright fresnel rim.
  float sheen = smoothstep(0.75, 0.0, length(q - vec2(-0.32, 0.42)));
  col += paper * sheen * sheen * 0.16;
  float fres = pow(1.0 - z, 3.0);
  vec3 rimCol = mix(mix(violet, rose, lit), paper, 0.25 + 0.3 * lit);
  col += fres * rimCol * (0.5 + 0.7 * u_energy);
  // While it thinks, a light travels round the rim, like a slow clock hand.
  float orbit = pow(max(0.0, cos(ang - u_time * 1.5 * u_wobble - 0.8)), 14.0);
  col += orbit * pow(1.0 - z, 1.6) * paper * 0.9 * u_think;

  // Muted and failed lose their colour and some of their light.
  float lum = dot(col, vec3(0.299, 0.587, 0.114));
  vec3 grey = vec3(lum) * vec3(0.86, 0.88, 1.0);
  col = mix(col, grey, clamp(u_mute * 0.92 + u_error * 0.65, 0.0, 1.0));
  col *= 1.0 - 0.38 * u_mute - 0.15 * u_error;
  // Premultiplied output must never be brighter than it is opaque.
  col = clamp(col, 0.0, 1.0);

  // A hairline of light exactly on the edge, the glass catching it.
  float line = exp(-abs(d) / (1.2 * u_px)) * (0.25 + 0.35 * u_energy);

  // Halo: light spilling round the sphere, warm on the lit side, violet on the
  // far side, fading to nothing well before the canvas edge.
  float out_ = max(d, 0.0);
  float halo = exp(-out_ * (10.0 - 4.0 * u_energy)) * (1.0 - smoothstep(0.6, 0.98, r));
  vec3 haloCol = mix(mix(indigo, violet, 0.6), mix(coral, orange, 0.35), smoothstep(-0.6, 0.8, dot(dir, normalize(vec2(-0.5, 0.86)))));
  haloCol = mix(haloCol, grey, clamp(u_mute + u_error * 0.6, 0.0, 1.0));
  float haloA = halo * (0.14 + 0.46 * u_energy) * (1.0 - inside) * (1.0 - 0.6 * u_mute);

  vec3 lineCol = mix(rimCol, paper, 0.5);
  vec3 rgb = col * inside + haloCol * haloA + lineCol * line * (1.0 - inside * 0.5);
  float a = clamp(inside + haloA + line * (1.0 - inside), 0.0, 1.0);
  rgb = min(rgb, vec3(a));
  gl_FragColor = vec4(rgb, a);
}
`;
