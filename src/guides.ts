import { listen } from "@tauri-apps/api/event";

type SnapGuide = {
  orientation: "vertical" | "horizontal";
  position: number;
};

const canvas = document.querySelector<HTMLCanvasElement>("#guides")!;
const ctx = canvas.getContext("2d")!;

function resize() {
  canvas.width = window.innerWidth * devicePixelRatio;
  canvas.height = window.innerHeight * devicePixelRatio;
  canvas.style.width = `${window.innerWidth}px`;
  canvas.style.height = `${window.innerHeight}px`;
  ctx.setTransform(devicePixelRatio, 0, 0, devicePixelRatio, 0, 0);
}

function draw(guides: SnapGuide[]) {
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  ctx.strokeStyle = "rgba(58, 160, 255, 0.85)";
  ctx.lineWidth = 1;
  for (const guide of guides) {
    ctx.beginPath();
    if (guide.orientation === "vertical") {
      ctx.moveTo(guide.position, 0);
      ctx.lineTo(guide.position, window.innerHeight);
    } else {
      ctx.moveTo(0, guide.position);
      ctx.lineTo(window.innerWidth, guide.position);
    }
    ctx.stroke();
  }
}

window.addEventListener("resize", resize);
resize();

void listen<SnapGuide[]>("layout://guides", (e) => {
  draw(e.payload ?? []);
});
