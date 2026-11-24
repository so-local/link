"use client";

import { FlowerScene, type SingleFlowerProps, type BackgroundProps } from "./scene";

export type { FlowerProps, SingleFlowerProps, BackgroundProps } from "./scene";
export function FlowerSvg(props: SingleFlowerProps) { return <FlowerScene {...props} single />; }
export function FlowerBackground({ fixed = true, ...props }: BackgroundProps) { return <FlowerScene {...props} fixed={fixed} />; }
