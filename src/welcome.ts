// First-run window (src-tauri/src/first_run.rs). Its capability allows only
// `welcome_setup` and `welcome_skip`; the host closes the window after either.
import { invoke } from "@tauri-apps/api/core";

type CopyReport = { written: string[]; skipped: string[] };

const setup = document.querySelector<HTMLButtonElement>("#setup")!;
const skip = document.querySelector<HTMLButtonElement>("#skip")!;
const status = document.querySelector<HTMLParagraphElement>("#status")!;

function busy(on: boolean) {
  setup.disabled = on;
  skip.disabled = on;
}

setup.addEventListener("click", async () => {
  busy(true);
  status.textContent = "Copying the example desktop…";
  try {
    const report = await invoke<CopyReport>("welcome_setup");
    status.textContent = `Done: ${report.written.length} files.`;
  } catch (err) {
    status.textContent = `Could not set it up: ${String(err)}`;
    busy(false);
  }
});

skip.addEventListener("click", async () => {
  busy(true);
  try {
    await invoke("welcome_skip");
  } catch (err) {
    status.textContent = String(err);
    busy(false);
  }
});

setup.focus();
