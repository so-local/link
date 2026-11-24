import assert from "node:assert/strict";
import { test } from "node:test";
import { shellCommands } from "./shell";

const split = (text: string) => shellCommands(text).map(command => command.text);

test("each independent terminal command copies separately and retains its exact text", () => {
  const text = "curl -fsSL https://local.link/install.sh | sh\nlnk up\nlnk agent start\nlnk agent chat";
  assert.deepEqual(split(text), text.split("\n"));
  assert.ok(shellCommands(text).every(command => command.copyable));
});

test("intentional chains and pipelines remain a single copy operation", () => {
  assert.deepEqual(split("npm run lint && npm test && npm run build\nlnk --help"), [
    "npm run lint && npm test && npm run build", "lnk --help",
  ]);
  for (const operator of ["&&", "||", "|"]) {
    const chain = `lnk agent list ${operator}\n  cat`;
    assert.deepEqual(split(`${chain}\nlnk --help`), [chain, "lnk --help"]);
  }
});

test("continued commands keep their arguments, indentation, and pipelines", () => {
  const command = "lnk sandbox tool add browser \\\n  --shared --add read \\\n  -- lnk-browser-tool serve";
  assert.deepEqual(split(`lnk module add browser\n${command}\nlnk agent permission add --tool browser`), [
    "lnk module add browser", command, "lnk agent permission add --tool browser",
  ]);
});

test("multiline quotes, substitutions, and arrays stay with their command", () => {
  for (const command of [
    "printf '%s\\n' 'first\nsecond'",
    'printf "%s\\n" "first\nsecond"',
    'value=$(\n  printf "%s" "$(echo main)"\n)',
    'value="$(\n  printf "%s" "$name"\n)"',
    'values=(\n  "main"\n  "work"\n)',
    'value=${name:-\nmain}',
    'value=`printf "%s"\nmain`',
  ]) assert.deepEqual(split(`${command}\nlnk --help`), [command, "lnk --help"]);
});

test("heredocs copy the complete file input without copying the following command", () => {
  for (const delimiter of ["EOF", "'EOF'", '"EOF"', "\\EOF"]) {
    const command = `cat > env.toml <<${delimiter}\nname = "app"\nversion = 1\nEOF`;
    assert.deepEqual(split(`${command}\nlnk env check --env env.toml`), [command, "lnk env check --env env.toml"]);
  }
  const command = "cat <<-EOF\n\tname = app\n\tEOF";
  assert.deepEqual(split(`${command}\nlnk --help`), [command, "lnk --help"]);
  const two = "cat <<FIRST <<'SECOND'\none\nFIRST\ntwo\nSECOND";
  assert.deepEqual(split(`${two}\nlnk --help`), [two, "lnk --help"]);
});

test("complete conditionals and loops keep nested statements together", () => {
  for (const command of [
    'if [ "$(lnk box list --json)" = "[]" ]; then\n  lnk box create\nelse\n  lnk box list\nfi',
    'for name in main work; do\n  if test "$name" = main; then\n    lnk agent show "$name"\n  fi\ndone',
    "while true; do\n  lnk agent list\n  break\ndone",
    "case $name in\n  for|while)\n    lnk agent list\n    ;;\nesac",
    'if true; then\n  cat <<EOF\nname = "main"\nEOF\nfi',
  ]) assert.deepEqual(split(`${command}\nlnk --help`), [command, "lnk --help"]);
});

test("subshells, groups, and functions remain complete statements", () => {
  for (const command of [
    "(\n  cd example\n  lnk --help\n)",
    "{\n  lnk --help\n  lnk agent list\n}",
    "example() {\n  lnk --help\n}",
    "[[\n  $name = main\n]]",
  ]) assert.deepEqual(split(`${command}\nlnk --help`), [command, "lnk --help"]);
});

test("comments and blank lines remain visible without independent copy buttons", () => {
  const commands = shellCommands("# Install first\nlnk up\n\n# Then start\nlnk agent start\n# Finished");
  assert.deepEqual(commands, [
    { text: "# Install first\nlnk up", copyable: true },
    { text: "\n# Then start\nlnk agent start\n# Finished", copyable: true },
  ]);
  assert.deepEqual(shellCommands("# No commands\n"), [{ text: "# No commands\n", copyable: false }]);
});

test("literal shell punctuation and reserved words in arguments never join independent commands", () => {
  for (const command of [
    "echo if for while done", "echo {", "echo name#tag", "echo '{'", 'printf "%s" "a|b"',
    "echo escaped\\#hash", "echo " + String.fromCharCode(92, 92), "cat <<< main", "printf '%s' '${name}'", "\\x",
  ]) assert.deepEqual(split(`${command}\nlnk --help`), [command, "lnk --help"]);
});
