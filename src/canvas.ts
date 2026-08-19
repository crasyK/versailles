import { invoke } from "@tauri-apps/api/core";

const canvas = document.querySelector<HTMLCanvasElement>("#board")!;
const toolbar = document.querySelector<HTMLDivElement>("#toolbar")!;
const ctx = canvas.getContext("2d")!;

type Tool = "pen" | "highlighter" | "eraser";
let tool: Tool = "pen";
let drawing = false;
let lastX = 0;
let lastY = 0;

function resize() {
  canvas.width = window.innerWidth * devicePixelRatio;
  canvas.height = window.innerHeight * devicePixelRatio;
  canvas.style.width = `${window.innerWidth}px`;
  canvas.style.height = `${window.innerHeight}px`;
  ctx.setTransform(devicePixelRatio, 0, 0, devicePixelRatio, 0, 0);
}

function renderToolbar() {
  toolbar.innerHTML = `
    <button class="cv-btn${tool === "pen" ? " on" : ""}" data-tool="pen">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M17 3l4 4L8 20l-5 1 1-5L17 3z"/></svg>
      Pen</button>
    <button class="cv-btn${tool === "highlighter" ? " on" : ""}" data-tool="highlighter">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M9 11l6 6M14 4l6 6-8.5 8.5H8l-3-3v-3.5L14 4z"/></svg>
      Highlighter</button>
    <button class="cv-btn${tool === "eraser" ? " on" : ""}" data-tool="eraser">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M7 21l-4-4 10-10 6 6-8 8H7zM13 21h8"/></svg>
      Eraser</button>
    <span class="cv-sep"></span>
    <button class="cv-btn" data-action="clear">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M3 6h18M8 6V4h8v2M6 6l1 14h10l1-14"/></svg>
      Clear</button>
    <button class="cv-btn" data-action="close">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>
      Close</button>
  `;
  toolbar.querySelectorAll<HTMLButtonElement>("[data-tool]").forEach((btn) => {
    btn.onclick = () => {
      tool = btn.dataset.tool as Tool;
      renderToolbar();
    };
  });
  toolbar.querySelector<HTMLButtonElement>("[data-action='clear']")!.onclick = () => {
    ctx.clearRect(0, 0, canvas.width, canvas.height);
  };
  toolbar.querySelector<HTMLButtonElement>("[data-action='close']")!.onclick = () => {
    void invoke("close_canvas");
  };
}

function strokeStyle() {
  if (tool === "pen") {
    ctx.globalCompositeOperation = "source-over";
    ctx.strokeStyle = "#ffffff";
    ctx.lineWidth = 2.5;
    ctx.globalAlpha = 1;
  } else if (tool === "highlighter") {
    ctx.globalCompositeOperation = "source-over";
    ctx.strokeStyle = "#ffe566";
    ctx.lineWidth = 16;
    ctx.globalAlpha = 0.35;
  } else {
    ctx.globalCompositeOperation = "destination-out";
    ctx.lineWidth = 24;
    ctx.globalAlpha = 1;
  }
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
}

canvas.addEventListener("pointerdown", (e) => {
  drawing = true;
  lastX = e.clientX;
  lastY = e.clientY;
});
canvas.addEventListener("pointerup", () => {
  drawing = false;
});
canvas.addEventListener("pointerleave", () => {
  drawing = false;
});
canvas.addEventListener("pointermove", (e) => {
  if (!drawing) return;
  strokeStyle();
  ctx.beginPath();
  ctx.moveTo(lastX, lastY);
  ctx.lineTo(e.clientX, e.clientY);
  ctx.stroke();
  lastX = e.clientX;
  lastY = e.clientY;
});

window.addEventListener("resize", resize);
window.addEventListener("keydown", (e) => {
  if (e.key === "Escape") void invoke("close_canvas");
});

resize();
renderToolbar();
