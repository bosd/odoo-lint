// Bundles the extension and its tests with esbuild: `vscode` is provided by
// the editor, everything else goes into the bundle.
import { build } from "esbuild";

const common = {
  bundle: true,
  platform: "node",
  format: "cjs",
  target: "node20",
  external: ["vscode"],
};

await build({
  ...common,
  entryPoints: ["src/extension.ts"],
  outfile: "dist/extension.js",
  minify: true,
});
await build({
  ...common,
  entryPoints: ["test/runTest.ts", "test/suite/index.ts", "test/wheel.test.ts"],
  outdir: "out",
  outbase: "test",
});
