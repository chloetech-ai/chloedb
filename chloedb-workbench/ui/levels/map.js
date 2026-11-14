// Placeholder: the map screen (SPEC.md, "Levels (third build)") is being built.
import { h, empty } from "./common.js";
export function mount(root) {
  root.replaceChildren(h("div", { class: "lv-wrap" }, empty("The map screen is on its way.")));
  return { update() {}, unmount() { root.replaceChildren(); } };
}
