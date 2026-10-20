// Runs the Tauri CLI (`pnpm tauri …`) with a clean environment when launched from a snap app's
// terminal, such as the VS Code snap on Ubuntu.
//
// Snap apps export variables like GTK_PATH that point at libraries built for the snap's own
// base (core20, glibc 2.31). The app links against the host's GTK/WebKit instead, so GTK loads
// the snap's modules, mixes two glibc versions, and the binary dies at startup with
// "symbol lookup error: … libpthread.so.0: undefined symbol: __libc_pthread_init".
// Outside a snap environment this changes nothing.
import { spawnSync } from "node:child_process";
import process from "node:process";

const SNAP_VARS = [
  "GTK_PATH",
  "GTK_EXE_PREFIX",
  "GTK_IM_MODULE_FILE",
  "GIO_MODULE_DIR",
  "GDK_PIXBUF_MODULE_FILE",
  "GDK_PIXBUF_MODULEDIR",
  "GSETTINGS_SCHEMA_DIR",
  "LOCPATH",
];

const env = { ...process.env };
if (process.platform === "linux" && env.SNAP) {
  for (const name of SNAP_VARS) delete env[name];
  // VS Code saves the values it overrode as <NAME>_VSCODE_SNAP_ORIG; put them back.
  for (const [name, value] of Object.entries(process.env)) {
    const original = name.match(/^(.+)_VSCODE_SNAP_ORIG$/)?.[1];
    if (!original) continue;
    if (value) env[original] = value;
    else delete env[original];
    delete env[name];
  }
}

const result = spawnSync("tauri", process.argv.slice(2), { stdio: "inherit", env, shell: process.platform === "win32" });
if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}
process.exit(result.status ?? 1);
