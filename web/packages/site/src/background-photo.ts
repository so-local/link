import settings from "./background-settings.json";

export type Photo = {
  name: string;
  position: string;
  wide?: string;
  responsive: { width: number; height: number; widths: number[] };
};

function positiveInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0;
}

/** Refuse malformed bucket data before it reaches image URLs or CSS. */
export function readPhotos(value: unknown): Photo[] {
  if (!value || typeof value !== "object" || !("backgrounds" in value) || !Array.isArray(value.backgrounds)) {
    throw new Error("Expected a background list");
  }
  return value.backgrounds.map((photo: unknown) => {
    if (!photo || typeof photo !== "object" || !("name" in photo) ||
        typeof photo.name !== "string" || !/^[a-z0-9][a-z0-9-]{0,40}$/.test(photo.name) ||
        !("position" in photo) || typeof photo.position !== "string" ||
        ("wide" in photo && typeof photo.wide !== "string")) throw new Error("Invalid background");
    if (!("responsive" in photo)) throw new Error("Expected background sizes");
    const r = photo.responsive;
    if (!r || typeof r !== "object" || !("width" in r) || !("height" in r) || !("widths" in r)) {
      throw new Error("Expected background dimensions and widths");
    }
    const { width, height, widths } = r;
    if (!positiveInteger(width) || !positiveInteger(height) || !Array.isArray(widths) || !widths.length ||
        !widths.every((w: unknown, i: number) => positiveInteger(w) && w <= width &&
          (i === 0 || w > widths[i - 1])) || widths.at(-1) !== width) {
      throw new Error("Invalid background sizes");
    }
    const responsive: Photo["responsive"] = { width, height, widths };
    return { name: photo.name, position: photo.position,
      ...("wide" in photo ? { wide: photo.wide as string } : {}), responsive };
  });
}

/** The source width needed by cover includes the taller parallax layer. */
export function imageSources(photo: Photo, at: string) {
  const { width, height, widths } = photo.responsive;
  return {
    src: `${at}/${width}.jpg`,
    srcSet: widths.map((w) => `${at}/${w}.jpg ${w}w`).join(", "),
    sizes: `max(100vw, ${settings.layerHeight * width / height}lvh)`,
  };
}
