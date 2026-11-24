// Markdown action titles keep an optional compact label beside their kind.
export type ActionTitle = { kind: "button" | "placeholder"; short?: string };

export function actionTitle(value: unknown): ActionTitle | undefined {
  if (typeof value !== "string") return;
  const [kind, short, ...extra] = value.split("|");
  if (kind !== "button" && kind !== "placeholder") return;
  if (extra.length || (short !== undefined && !short.trim())) {
    throw new Error("action titles use button|Short Label or placeholder|Short Label");
  }
  return short === undefined ? { kind } : { kind, short: short.trim() };
}
