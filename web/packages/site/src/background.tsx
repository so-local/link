"use client";

// The site-wide background: the day's photo, drifting slightly as the
// page scrolls. The photos are in the public bucket (bucket.ts), each a
// folder of its own of responsive images, and every site shows the
// list's day-th in turn, the same everywhere. A tiny copy shows blurred
// at once, and the photo fades in over it when it arrives; until the
// list is read, the page's own dark colour shows (globals.css).

import { useEffect, useState } from "react";
import { bucket } from "./bucket";
import Parallax from "./parallax";
import { imageSources, readPhotos, type Photo } from "./background-photo";


/** The day's photo: the list's day-th, counting days in UTC. */
function today(list: Photo[]): Photo | undefined {
  return list[Math.floor(Date.now() / 86_400_000) % list.length];
}

export default function Background() {
  const [photo, setPhoto] = useState<Photo>();
  const [loaded, setLoaded] = useState(false);
  useEffect(() => {
    let gone = false;
    fetch(`${bucket}/backgrounds/backgrounds.json`)
      .then((r) => (r.ok ? r.json() : Promise.reject(r.status)))
      .then((value: unknown) => {
        const list = readPhotos(value);
        const found = list.length ? today(list) : undefined;
        if (!gone && found) setPhoto(found);
      })
      .catch(() => {});
    return () => {
      gone = true;
    };
  }, []);
  if (!photo) return null;
  const at = `${bucket}/backgrounds/${photo.name}`;
  // Where the photo is centred: on a narrow screen, then a wide one.
  const place = { "--at": photo.position, "--wide": photo.wide ?? photo.position } as React.CSSProperties;
  return (
    <div aria-hidden className="fixed inset-0 -z-10 overflow-hidden">
      <Parallax>
        <div
          style={{ ...place, backgroundImage: `url(${at}/blur.jpg)` }}
          className="absolute inset-0 scale-110 bg-cover bg-[position:var(--at)] blur-2xl lg:bg-[position:var(--wide)]"
        />
        {/* eslint-disable-next-line @next/next/no-img-element -- the bucket's own sizes, not Next's */}
        <img
          {...imageSources(photo, at)}
          alt=""
          onLoad={() => setLoaded(true)}
          style={place}
          className={`absolute inset-0 h-full w-full object-cover object-[var(--at)] transition-opacity duration-700 lg:object-[var(--wide)] ${loaded ? "opacity-100" : "opacity-0"}`}
        />
      </Parallax>
    </div>
  );
}
