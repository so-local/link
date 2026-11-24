# Flowers

> Sunflowers Behind Every Page

Render a configurable animated sunflower or a field background. Both are React Scalable Vector Graphics (SVG) components for browser hosts, using the same flower shapes and circular-stem geometry.

## Render a Flower

Import `FlowerSvg` from `@link-flowers/browser` and pass partial flower settings. The full flower moves in the wind and responds to the mouse, while head framing gives a static icon. Set a title for meaningful artwork, or omit it for a decorative image hidden from assistive technology.

```tsx
import { FlowerSvg } from "@link-flowers/browser";

export function Sunflower() {
  return <FlowerSvg
    title="Sunflower"
    flower={{ stem: { thickness: 9 }, petals: { innerWidth: 35 } }}
    settings={{ windStrength: 80 }}
    style={{ width: 300, height: 360 }}
  />;
}
```

## Render a Background

`FlowerBackground` fills the viewport by default and leaves foreground clicks alone. Its height is the large viewport (`100lvh`), so it never resizes as a phone browser shows or hides its toolbars. A field sized to the visible window redraws every flower on each frame of that animation, which jitters at the end of a momentum scroll on iOS. Mount it once in a persistent layout, beside the changing page content. Route changes keep the same SVG node, animation clock and wind phases. For an embedded example, set `fixed={false}` and supply a container height.

```tsx
import { FlowerBackground } from "@link-flowers/browser";

export function Background() {
  return <FlowerBackground settings={{ flowerCount: 36, flowerStrength: 40 }} />;
}
```

## Configure the Components

Both components take `flower`, `settings`, `paused`, `interactive`, `pointerTarget`, `className`, `style` and `title`. `FlowerSvg` also accepts `framing="head"` or `framing="full"`, and `FlowerBackground` accepts `fixed`. A `pointerTarget` ref selects the host container that observes movement. Fixed backgrounds observe the document by default. Touch input does not bend the flowers.

### Flower Settings

`FlowerOptions` accepts partial groups for petals, center, stem, leaves and canvas framing. Settings include colors, petal arrangement and dimensions, stem length/radius/sweep, individual leaves and transparent framing. The [typed definition and defaults](../../../../../shared/settings.ts) own the complete field list. Invalid dimensions, nonfinite numbers and invalid colors are refused before rendering.

### Scene Settings

`SceneSettings` owns count, bloom and leaf size, size variance, stem width, leaf-angle variance, wind strength/speed, packing, overlap, field/hill position, sky response, mouse push/reach, easing and seed. All are optional props with shared defaults. Defaults preserve 36 flowers, 30px sky response, 40px flower push, 400px reach, 340ms easing, 35% packing and 0px hill drop. The seed keeps placements stable across renders.

## Motion and Lifecycle

The stem stays anchored at its root, and the head center and leaves follow the same circular arc. The head inherits the stem tangent without independent roll. One controller updates SVG attributes per scene without React renders on each frame. Layout is solved at construction or resize. Pause, reduced motion, document visibility and offscreen detection stop frame work. Unmount removes listeners and observers. Resuming keeps elapsed animation time without a jump.

### Settling While You Read

A fixed background comes to rest while the page is scrolled down, so nothing
moves behind the text being read. Once the page scrolls past its top, the
wind slows to a stop over 1.2 seconds, and then no frames are drawn at all.
Back at the top, it eases up again over the same time. The page is the window,
or an element at least half the window's height that scrolls, such as an
app's own scrolling view. A box scrolling inside the page, such as a wide code
block, never counts. The time and the distance are `settleTime` and
`scrollRest` in the shared package's `motionConstants`.

## Browser Support

The components require browser SVG, requestAnimationFrame, ResizeObserver, IntersectionObserver and media-query events. They prerender without browser globals, then animate after mounting. A native host imports `@link-flowers/shared` alone, never the DOM artwork. Rendering cost includes changing SVG paths and paint, so callback timings from a software browser do not establish GPU or mobile performance. The [design choices](decisions.md) explain these boundaries.

<!-- docs-tree:start -->

## Docs

- [Decisions](decisions.md): Why flower rendering and animation share one browser implementation

<!-- docs-tree:end -->
