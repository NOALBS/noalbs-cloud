import {
  Box,
  Camera,
  Clapperboard,
  Folder,
  Globe,
  Image,
  Layers,
  type LucideIcon,
  Monitor,
  Type,
  Volume2,
} from "lucide-react";
import type { SourceKind } from "@/lib/obs-sources";

/** A source's icon, by what it is. */
export const SOURCE_ICONS: Record<SourceKind, LucideIcon> = {
  scene: Layers,
  group: Folder,
  media: Clapperboard,
  camera: Camera,
  capture: Monitor,
  audio: Volume2,
  browser: Globe,
  text: Type,
  image: Image,
  other: Box,
};
