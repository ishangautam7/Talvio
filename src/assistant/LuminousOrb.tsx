import React, { useEffect, useRef, useState } from "react";
import {
  approachWeights,
  flowSpeed,
  followLevel,
  orbEnergy,
  targetWeights,
  type OrbMode,
  type OrbWeights,
} from "./orbMotion";
import { ORB_FRAGMENT, ORB_VERTEX } from "./orbShader";
import "./VoiceOrb.css";

/**
 * The assistant's presence: a lit glass sphere of flowing coral and indigo
 * light that listens, thinks and speaks.
 *
 * Drawn by one WebGL fragment shader (`orbShader.ts`) on a transparent canvas
 * twice the sphere's size, so its halo has room. Motion is decided in
 * `orbMotion.ts`; this file only runs the frame loop. Where WebGL is missing
 * or fails (tests, a broken driver), the CSS orb underneath — the same
 * palette, painted with gradients — is what shows, so there is never a hole.
 *
 * The level is read every frame from `levelSource` when given, so a voice at
 * 30 updates a second moves the orb without re-rendering React.
 */
export interface LuminousOrbProps {
  mode: OrbMode;
  /** Diameter of the sphere in CSS px. The element is twice this, for the halo. */
  size: number;
  /** A voice level 0..1, when it changes slowly enough to pass as a prop. */
  level?: number;
  /** Read once per frame; wins over `level`. */
  levelSource?: () => number;
  className?: string;
}

const prefersReducedMotion = () =>
  typeof window !== "undefined" &&
  typeof window.matchMedia === "function" &&
  window.matchMedia("(prefers-reduced-motion: reduce)").matches;

interface GlProgram {
  gl: WebGLRenderingContext;
  uniforms: Record<string, WebGLUniformLocation | null>;
  dispose: () => void;
}

const UNIFORMS = [
  "u_px",
  "u_time",
  "u_flow",
  "u_level",
  "u_energy",
  "u_listen",
  "u_think",
  "u_speak",
  "u_mute",
  "u_error",
  "u_wobble",
] as const;

function compile(
  gl: WebGLRenderingContext,
  type: number,
  source: string,
): WebGLShader | null {
  const shader = gl.createShader(type);
  if (!shader) return null;
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    console.warn("Orb shader failed:", gl.getShaderInfoLog(shader));
    gl.deleteShader(shader);
    return null;
  }
  return shader;
}

function createProgram(canvas: HTMLCanvasElement): GlProgram | null {
  let gl: WebGLRenderingContext | null = null;
  try {
    gl = canvas.getContext("webgl", {
      alpha: true,
      premultipliedAlpha: true,
      antialias: false,
      depth: false,
      stencil: false,
      preserveDrawingBuffer: false,
      powerPreference: "low-power",
    }) as WebGLRenderingContext | null;
  } catch {
    gl = null;
  }
  if (!gl || gl.isContextLost()) return null;
  const vs = compile(gl, gl.VERTEX_SHADER, ORB_VERTEX);
  const fs = compile(gl, gl.FRAGMENT_SHADER, ORB_FRAGMENT);
  if (!vs || !fs) return null;
  const program = gl.createProgram();
  if (!program) return null;
  gl.attachShader(program, vs);
  gl.attachShader(program, fs);
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    console.warn("Orb program failed:", gl.getProgramInfoLog(program));
    return null;
  }
  gl.useProgram(program);
  const buffer = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  gl.bufferData(
    gl.ARRAY_BUFFER,
    new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]),
    gl.STATIC_DRAW,
  );
  const position = gl.getAttribLocation(program, "a_pos");
  gl.enableVertexAttribArray(position);
  gl.vertexAttribPointer(position, 2, gl.FLOAT, false, 0, 0);
  gl.clearColor(0, 0, 0, 0);
  const uniforms: GlProgram["uniforms"] = {};
  for (const name of UNIFORMS)
    uniforms[name] = gl.getUniformLocation(program, name);
  const context = gl;
  return {
    gl,
    uniforms,
    dispose: () => {
      context.deleteBuffer(buffer);
      context.deleteProgram(program);
      context.deleteShader(vs);
      context.deleteShader(fs);
      // The context itself is left to the canvas: losing it here would leave
      // a remount on the same canvas (React's StrictMode does exactly that)
      // with a dead context. It goes when the canvas does.
    },
  };
}

export const LuminousOrb: React.FC<LuminousOrbProps> = ({
  mode,
  size,
  level = 0,
  levelSource,
  className,
}) => {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [lit, setLit] = useState(false);
  // The loop reads these every frame; props only update them.
  const modeRef = useRef(mode);
  modeRef.current = mode;
  const levelRef = useRef(level);
  levelRef.current = level;
  const sourceRef = useRef(levelSource);
  sourceRef.current = levelSource;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || typeof canvas.getContext !== "function") return;
    const program = createProgram(canvas);
    if (!program) return;
    const { gl, uniforms } = program;
    setLit(true);

    let frame = 0;
    let last = performance.now();
    const start = last;
    let flow = Math.random() * 40;
    let smoothed = 0;
    let weights: OrbWeights = targetWeights(modeRef.current);
    const reduced = prefersReducedMotion();

    const draw = (now: number) => {
      frame = requestAnimationFrame(draw);
      if (typeof document !== "undefined" && document.hidden) return;
      const dt = Math.min(0.1, Math.max(0, (now - last) / 1000));
      last = now;
      const time = (now - start) / 1000;
      const input = sourceRef.current ? sourceRef.current() : levelRef.current;
      smoothed = followLevel(smoothed, input, dt);
      weights = approachWeights(weights, targetWeights(modeRef.current), dt);
      flow += dt * flowSpeed(weights, smoothed, reduced);
      const energy = orbEnergy(weights, smoothed, time, reduced);

      const dpr = Math.min(2, window.devicePixelRatio || 1);
      const px = Math.round(canvas.clientWidth * dpr) || size * 2;
      if (canvas.width !== px || canvas.height !== px) {
        canvas.width = px;
        canvas.height = px;
        gl.viewport(0, 0, px, px);
      }
      gl.clear(gl.COLOR_BUFFER_BIT);
      gl.uniform1f(uniforms.u_px, 2 / px);
      gl.uniform1f(uniforms.u_time, reduced ? 0 : time);
      gl.uniform1f(uniforms.u_flow, flow);
      gl.uniform1f(uniforms.u_level, reduced ? smoothed * 0.4 : smoothed);
      gl.uniform1f(uniforms.u_energy, energy);
      gl.uniform1f(uniforms.u_listen, weights.listen);
      gl.uniform1f(uniforms.u_think, weights.think);
      gl.uniform1f(uniforms.u_speak, weights.speak);
      gl.uniform1f(uniforms.u_mute, weights.mute);
      gl.uniform1f(uniforms.u_error, weights.error);
      gl.uniform1f(uniforms.u_wobble, reduced ? 0 : 1);
      gl.drawArrays(gl.TRIANGLES, 0, 6);
    };
    frame = requestAnimationFrame(draw);

    const lost = (event: Event) => {
      event.preventDefault();
      cancelAnimationFrame(frame);
      setLit(false);
    };
    canvas.addEventListener("webglcontextlost", lost);
    return () => {
      cancelAnimationFrame(frame);
      canvas.removeEventListener("webglcontextlost", lost);
      program.dispose();
      setLit(false);
    };
  }, [size]);

  return (
    <div
      className={`luminous-orb ${mode}${lit ? " is-lit" : ""}${
        className ? ` ${className}` : ""
      }`}
      style={{ "--orb-d": `${size}px` } as React.CSSProperties}
      aria-hidden="true"
    >
      {/* The painted orb: what shows until (or unless) the canvas lights. */}
      <div
        className={`voice-orb luminous-orb-fallback ${mode}`}
        style={
          {
            "--voice-energy":
              mode === "listening" || mode === "speaking" ? level : 0,
          } as React.CSSProperties
        }
      >
        <div className="voice-orb-body">
          <span className="voice-orb-flow" />
          <span className="voice-orb-light" />
        </div>
      </div>
      <canvas ref={canvasRef} className="luminous-orb-canvas" />
    </div>
  );
};

export default LuminousOrb;
