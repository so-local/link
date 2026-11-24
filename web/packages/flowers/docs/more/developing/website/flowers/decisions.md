# Decisions

Why flower rendering and animation share one browser implementation. Component usage and settings belong to [Flowers](README.md).

## Shared SVG Geometry

The single flower and background use the same petal, leaf and circular-arc functions. Repeated shapes use SVG definitions, while each plant owns its stem and attachment transforms. The head center is the arc endpoint and its angle is the endpoint tangent. Sharing the geometry prevents independent head rotation or a gap at the attachment when wind and pointer movement combine.

## Persistent Layouts

Backgrounds live beside navigation content in each host's persistent root layout. The animation clock belongs to the mounted scene, with no route key or dependency on the current URL. Route transitions therefore retain the SVG and wind phases instead of recreating the field. The websites' shared layout mounts the field background by default and keeps a background slot, so a site can supply other artwork.

## Animation Controller

Native JavaScript updates the changing circular arc and a small number of transforms in one frame loop. React handles configuration and scene structure. This avoids a dependency for geometry that already needs custom calculation. Instance-local definitions and rounded SVG markup keep multiple scenes independent and server/browser rendering consistent. Changing SVG paths can cause paint, so rendering performance still needs measurement on the target device.

## Browser Boundary

Portable settings and geometry live in `@link-flowers/shared`. DOM rendering and lifecycle live in `@link-flowers/browser`, with no framework imports. A host with native and web builds mounts the browser component from its web build alone. The browser implementation does not imply native SVG support.
