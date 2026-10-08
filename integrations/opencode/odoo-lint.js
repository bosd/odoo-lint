// odoo-lint for OpenCode: lints every Odoo addon file the agent edits and
// appends the findings to the edit's result, so the model sees and fixes them.
//
// Install: copy this file to `.opencode/plugins/` in your project, or to
// `~/.config/opencode/plugins/` for all projects. Needs `odl` in the
// project's `.venv` or on PATH (`uv tool install odoo-linter`).

const EDIT_TOOLS = new Set(["edit", "write", "multiedit"]);
const LINTED = /\.(py|po|pot)$/;

export const OdooLint = async ({ $, directory }) => {
  const local = `${directory}/.venv/bin/odl`;
  const odl = (await Bun.file(local).exists()) ? local : "odl";

  return {
    "tool.execute.after": async (input, output) => {
      const file = input.args?.filePath;
      if (!EDIT_TOOLS.has(input.tool) || !file || !LINTED.test(file)) return;
      // `odl hook` reads a Claude Code style hook event.
      const event = JSON.stringify({
        hook_event_name: "PostToolUse",
        cwd: directory,
        tool_input: { file_path: file },
      });
      const result = await $`${odl} hook < ${new Response(event)}`
        .quiet()
        .nothrow();
      const text = result.stdout.toString().trim();
      if (result.exitCode !== 0 || !text) return;
      try {
        const context = JSON.parse(text).hookSpecificOutput?.additionalContext;
        if (context) output.output = `${output.output}\n\n${context}`;
      } catch {
        // Not odl's output: leave the result alone.
      }
    },
  };
};
