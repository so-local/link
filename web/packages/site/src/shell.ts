/** A complete shell statement, with comments kept beside the command they explain. */
export type ShellCommand = { text: string; copyable: boolean };
type Quote = "'" | '"' | "`";
type Frame = { end: string; quote?: Quote; shell: boolean };
type HereDoc = { end: string; tabs: boolean };
type Block = { end: string; casePart?: "header" | "pattern" | "body" };

/** Split terminal examples at complete lines, preserving explicit shell chains and bodies. */
export function shellCommands(text: string): ShellCommand[] {
  const commands: ShellCommand[] = [], lines: string[] = [];
  const frames: Frame[] = [], blocks: Block[] = [], documents: HereDoc[] = [];
  let quote: Quote | undefined, copyable = false, continued = false;
  const finish = () => {
    commands.push({ text: lines.join("\n"), copyable });
    lines.length = 0;
    copyable = false;
  };
  for (const line of text.split("\n")) {
    lines.push(line);
    if (documents.length) {
      const document = documents[0];
      if ((document.tabs ? line.replace(/^\t+/, "") : line).replace(/\r$/, "") === document.end) documents.shift();
      if (!documents.length && !blocks.length && !frames.length && !quote && !continued) finish();
      continue;
    }
    let start = true;
    continued = false;
    for (let i = 0; i < line.length; i++) {
      const char = line[i], next = line[i + 1];
      if (char === "\\" && quote !== "'") {
        copyable = true;
        if (i === line.length - 1) continued = true;
        i++;
        continue;
      }
      if (quote === "'" || quote === "`") {
        if (char === quote) quote = undefined;
        continue;
      }
      if (char === "$" && (next === "(" || next === "{")) {
        frames.push({ end: next === "(" ? ")" : "}", quote, shell: next === "(" && line[i + 2] !== "(" });
        quote = undefined;
        start = true;
        i++;
        continue;
      }
      if (quote === '"') {
        if (char === quote) quote = undefined;
        continue;
      }
      if (char === "#" && (i === 0 || /[\s;|&()]/.test(line[i - 1]))) break;
      if (/\s/.test(char)) continue;
      if (char === "'" || char === '"' || char === "`") {
        quote = char;
        copyable = true;
        start = false;
        continued = false;
        continue;
      }
      if (char === "<" && next === "<" && line[i + 2] !== "<" && line[i - 1] !== "<") {
        const match = line.slice(i + 2).match(/^(-?)[ \t]*(?:'([^']*)'|"([^"]*)"|([^\s;&|<>]+))/);
        if (match) {
          copyable = true;
          continued = false;
          documents.push({ end: (match[2] ?? match[3] ?? match[4]).replace(/\\(.)/g, "$1"), tabs: match[1] === "-" });
          i += match[0].length + 1;
          continue;
        }
      }
      const brace = char === "{" && (start || /\)\s*$/.test(line.slice(0, i))) && (!next || /\s/.test(next));
      if (char === ")" && blocks.at(-1)?.casePart === "pattern" && !frames.length) {
        blocks.at(-1)!.casePart = "body";
        start = true;
      } else if ((char === "(" && blocks.at(-1)?.casePart !== "pattern") || brace || (char === "[" && next === "[")) {
        frames.push({ end: char === "(" ? ")" : char === "{" ? "}" : "]]", shell: char !== "[" });
        if (char === "[") i++;
        start = true;
      } else if (frames.at(-1)?.end === char || (char === "]" && next === "]" && frames.at(-1)?.end === "]]")) {
        quote = frames.pop()!.quote;
        if (char === "]") i++;
        start = false;
      } else if (char === ";" || char === "&" || char === "|") {
        if (char === ";" && (next === ";" || next === "&") && blocks.at(-1)?.casePart === "body") blocks.at(-1)!.casePart = "pattern";
        continued = char === "|" || (char === "&" && next === "&");
        if (next === char) i++;
        start = true;
        continue;
      } else {
        const word = line.slice(i).match(/^[a-zA-Z_][a-zA-Z_0-9-]*(?=$|[\s;|&()<>])/);
        if (word) {
          const token = word[0];
          const block = blocks.at(-1);
          if (block?.casePart === "header" && token === "in") block.casePart = "pattern";
          else if (start && (frames.at(-1)?.shell ?? true) && (block?.casePart !== "pattern" || token === "esac")) {
            if (token === "if") blocks.push({ end: "fi" });
            else if (["for", "while", "until", "select"].includes(token)) blocks.push({ end: "done" });
            else if (token === "case") blocks.push({ end: "esac", casePart: "header" });
            else if (token === block?.end) blocks.pop();
          }
          start = ["if", "then", "else", "elif", "do", "while", "until"].includes(token) && start;
          i += token.length - 1;
        } else start = char === ")";
      }
      copyable = true;
      continued = false;
    }
    if (copyable && !quote && !frames.length && !blocks.length && !documents.length && !continued) finish();
  }
  if (lines.length) {
    if (!copyable && commands.length) commands.at(-1)!.text += `\n${lines.join("\n")}`;
    else finish();
  }
  return commands;
}
