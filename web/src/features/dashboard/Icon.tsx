import type { ComponentType } from 'react';
import {
  Activity,
  AlertTriangle,
  BarChart,
  Check,
  Clock,
  Database,
  Eye,
  Filter,
  Layers,
  Lock,
  Shield,
  Sliders,
  TrendingDown,
  TrendingUp,
  Zap,
} from 'slimicons';

/**
 * The one icon set (slimicons, MIT: 24 px grid, 1.5 px stroke). Named by meaning
 * rather than by glyph so a page reads consistently, and decorative by default —
 * an icon never carries information on its own (the label does).
 */
const GLYPHS = {
  runtime: Activity,
  activity: Activity,
  payloads: Shield,
  quality: Check,
  datasets: Layers,
  layers: Layers,
  spend: BarChart,
  audit: Filter,
  prune: Sliders,
  lint: Eye,
  pii: Lock,
  cost: Zap,
  latency: Clock,
  warning: AlertTriangle,
  up: TrendingUp,
  down: TrendingDown,
  ok: Check,
  store: Database,
} satisfies Record<string, ComponentType<{ size?: number; color?: string; className?: string }>>;

export type IconName = keyof typeof GLYPHS;

export const Icon = (props: { name: IconName; size?: number; className?: string }) => {
  const Glyph = GLYPHS[props.name];

  return (
    <Glyph
      size={props.size ?? 16}
      className={props.className}
      color="currentColor"
    />
  );
};
