/** slimicons ships no types: it is a set of SVG React components (MIT). */
declare module 'slimicons' {
  import type { ComponentType } from 'react';

  export type SlimIconProps = {
    size?: number;
    color?: string;
    className?: string;
    [key: string]: unknown;
  };

  export const Activity: ComponentType<SlimIconProps>;
  export const AlertTriangle: ComponentType<SlimIconProps>;
  export const BarChart: ComponentType<SlimIconProps>;
  export const Check: ComponentType<SlimIconProps>;
  export const Clock: ComponentType<SlimIconProps>;
  export const Database: ComponentType<SlimIconProps>;
  export const Eye: ComponentType<SlimIconProps>;
  export const Filter: ComponentType<SlimIconProps>;
  export const Layers: ComponentType<SlimIconProps>;
  export const Lock: ComponentType<SlimIconProps>;
  export const Shield: ComponentType<SlimIconProps>;
  export const Sliders: ComponentType<SlimIconProps>;
  export const TrendingDown: ComponentType<SlimIconProps>;
  export const TrendingUp: ComponentType<SlimIconProps>;
  export const Zap: ComponentType<SlimIconProps>;
}
