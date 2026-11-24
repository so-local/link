"use client";

// Each complete command in a terminal example has its own copy button.

import { useId, useState, type ReactNode } from "react";
import { shellCommands, type ShellCommand } from "./shell";

export default function Command({
  text,
  copy,
  copied,
}: {
  text: string;
  copy: string;
  copied: string;
}) {
  return (
    <div data-terminal className="not-prose my-4 min-w-0 max-w-full divide-y divide-zinc-900/10 rounded-lg bg-zinc-900/5 ring-1 ring-zinc-900/10">
      {shellCommands(text).map((command, index) => <CommandRow key={index} command={command} copy={copy} copied={copied} />)}
    </div>
  );
}

function CommandRow({ command, copy: copyWord, copied: copiedWord }: { command: ShellCommand; copy: string; copied: string }) {
  const id = useId();
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(command.text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // No clipboard (an old browser or an insecure page): select by hand.
    }
  };
  // A long command wraps between its words, never inside one unless a word
  // is wider than the whole box, and each line it wraps onto is indented
  // under its first. A pipe stays with the word before it, so an address
  // and the `| sh` it feeds read as one line. On a phone the type is a size
  // smaller, so `curl -fsSL` and then its address fit on two lines with
  // nothing scrolled out of sight. The Copy button sits beside the command,
  // never over it: on a phone it floats beside the first line, so the lines
  // under it take the box's whole width, and wider, it's a column of its
  // own, so a box sized to its command fits it on one line. What's copied
  // is the command as written.
  return (
    <div className="py-3 pr-2 pl-3 sm:flex sm:items-start sm:gap-2 sm:pl-4">
      {command.copyable && <button
        type="button"
        aria-describedby={id}
        onClick={copy}
        className="float-right ml-2 h-6 shrink-0 sm:order-last sm:float-none sm:ml-auto select-none rounded-md bg-white/90 px-2 font-sans text-xs leading-6 text-zinc-600 ring-1 ring-zinc-900/10 hover:text-zinc-900"
      >
        {copied ? copiedWord : copyWord}
      </button>}
      <pre className="pl-[2ch] -indent-[2ch] sm:min-w-0 sm:flex-1 font-mono text-xs leading-6 break-words whitespace-pre-wrap text-zinc-900 sm:text-sm">
        <code id={id}>{shown(command.text)}</code>
      </pre>
    </div>
  );
}

/**
 * A command as shown: each pipe kept on one line with the word before it
 * and the one after, by a span that doesn't wrap, so the spaces stay
 * ordinary ones whether it's copied with the button or selected by hand.
 */
function shown(text: string): ReactNode[] {
  return text.split(/(\S+ \| \S+)/).map((part, i) => (i % 2 ? <span key={i} className="whitespace-nowrap">{part}</span> : part));
}
