"use client";

import { useState } from "react";

/** The same quiet artwork fills an absent cover and a failed image request. */
export default function Cover({ title, src }: { title: string; src?: string }) {
  const [failed, setFailed] = useState<string>();
  return (
    <span className="page-card-cover" aria-hidden="true">
      <svg viewBox="0 0 80 64" fill="none" className="page-card-mark">
        <rect x="13" y="6" width="40" height="48" rx="6" stroke="currentColor" strokeWidth="2" />
        <path d="M24 19h18M24 28h18M24 37h11" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
        <rect x="48" y="31" width="21" height="21" rx="10.5" fill="currentColor" />
        <path d="m56 37 5 5-5 5" stroke="white" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
      {src && failed !== src && (
        // eslint-disable-next-line @next/next/no-img-element -- public bucket images need no image-optimization host or server
        <img src={src} alt="" width={1200} height={630} loading="lazy"
          ref={image => { if (image?.complete && !image.naturalWidth) setFailed(src); }}
          onError={() => setFailed(src)} />
      )}
      <span className="sr-only">{title}</span>
    </span>
  );
}
