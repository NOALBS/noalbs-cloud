import { t } from "@lingui/core/macro";
import type { ComponentProps } from "react";
import { ScenePicker } from "@/components/scene-picker";
export function SceneChoice(props: ComponentProps<typeof ScenePicker>) {
  const name = props.obs?.scenes.find((scene) => scene.uuid === props.value)?.name;
  const description = props.value
    ? name
      ? t`Uses ${name}.`
      : props.obs
        ? t`This scene is no longer in OBS.`
        : t`Connect OBS to identify this scene.`
    : t`No scene is selected; the fallback applies.`;
  return (
    <div className="flex w-full min-w-0 flex-col gap-1 sm:w-56">
      <ScenePicker {...props} />
      <p className="text-xs text-muted-foreground">{description}</p>
    </div>
  );
}
