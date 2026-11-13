// Draws Szeptusie, the reaction and sticker figures from figures-data.js,
// as SVG animated with SMIL, so nothing needs inline styles.
"use strict";

const FIGURES = Object.fromEntries(FIGURE_DATA.figures.map((figure) => [figure.id, figure]));
const REACTIONS = FIGURE_DATA.reactions;
const STICKERS = FIGURE_DATA.stickers;
const SVG_NS = "http://www.w3.org/2000/svg";
const REDUCED_MOTION = window.matchMedia("(prefers-reduced-motion: reduce)");

function svgNode(tag, attrs = {}, ...children) {
  const node = document.createElementNS(SVG_NS, tag);
  for (const [name, value] of Object.entries(attrs)) {
    if (value !== null && value !== undefined) node.setAttribute(name, String(value));
  }
  node.append(...children);
  return node;
}

function shapeNode(shape) {
  const paint = {
    fill: shape.fill || "none",
    stroke: shape.stroke || "none",
    "stroke-width": shape.stroke ? shape.w : null,
    "stroke-linecap": "round",
    "stroke-linejoin": "round",
  };
  const n = shape.n;
  switch (shape.t) {
    case "c":
      return svgNode("circle", { cx: n[0], cy: n[1], r: n[2], ...paint });
    case "e":
      return svgNode("ellipse", { cx: n[0], cy: n[1], rx: n[2], ry: n[3], ...paint });
    case "r":
      return svgNode("rect", { x: n[0], y: n[1], width: n[2], height: n[3], rx: n[4], ...paint });
    default:
      return svgNode("path", { d: shape.d, "fill-rule": shape.evenodd ? "evenodd" : null, ...paint });
  }
}

const EASE = ".45 0 .55 1";

// The SMIL elements for one animation; mirrors the Kotlin version in Figures.kt.
function animationNodes(anim) {
  const [kind, period, amount, phase] = anim;
  const timing = {
    dur: `${period}ms`,
    begin: `${-Math.round(phase * period)}ms`,
    repeatCount: "indefinite",
  };
  const transform = (type, values, keyTimes, eased) =>
    svgNode("animateTransform", {
      attributeName: "transform",
      type,
      values,
      keyTimes,
      calcMode: eased ? "spline" : "linear",
      keySplines: eased ? keyTimes.split(";").slice(1).map(() => EASE).join(";") : null,
      ...timing,
    });
  switch (kind) {
    case "bob":
      return [transform("translate", `0 0;0 ${-amount};0 0`, "0;0.5;1", true)];
    case "sway":
      return [transform("rotate", `0;${amount};0;${-amount};0`, "0;0.25;0.5;0.75;1", true)];
    case "spin":
      return [transform("rotate", "0;360", "0;1", false)];
    case "pulse":
      return [transform("scale", `1;${1 + amount};1`, "0;0.5;1", true)];
    case "blink":
      return [transform("scale", "1 1;1 1;1 0.1;1 1", "0;0.9;0.95;1", false)];
    case "shake":
      return [transform("translate", `0 0;${-amount} 0;${amount} 0;${-amount} 0;${amount} 0;0 0`,
        "0;0.2;0.4;0.6;0.8;1", false)];
    case "hop":
      return [transform("translate", `0 0;0 ${-amount};0 0;0 0`, "0;0.35;0.7;1", true)];
    case "rise":
      return [
        transform("translate", `0 0;0 ${-amount}`, "0;1", false),
        svgNode("animate", { attributeName: "opacity", values: "0;1;1;0", keyTimes: "0;0.15;0.7;1", ...timing }),
      ];
    default:
      return [];
  }
}

// An <svg> of the figure `id`, `size` pixels wide. With `animate` false it
// stands still until resumed with svg.unpauseAnimations().
function figureSvg(id, size, { animate = true, label = null } = {}) {
  const figure = FIGURES[id];
  const svg = svgNode("svg", {
    viewBox: "0 0 64 64",
    width: size,
    height: size,
    class: "figure",
    role: label ? "img" : null,
    "aria-label": label,
    "aria-hidden": label ? null : "true",
  });
  if (!figure) return svg;
  for (const part of figure.parts) {
    const [ox, oy] = part.origin;
    const moving = svgNode("g", {});
    if (part.anim) moving.append(...animationNodes(part.anim));
    moving.append(svgNode("g", { transform: `translate(${-ox} ${-oy})` }, ...part.shapes.map(shapeNode)));
    svg.append(svgNode("g", { transform: `translate(${ox} ${oy}) rotate(${part.rotate})` }, moving));
  }
  if (!animate || REDUCED_MOTION.matches) svg.pauseAnimations?.();
  return svg;
}

function figureName(id) {
  return FIGURES[id]?.name ?? id;
}

function stickerCaption(id) {
  return FIGURES[id]?.caption ?? "";
}
