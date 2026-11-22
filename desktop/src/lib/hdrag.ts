// Horizontal drag-handle action shared by the library column dividers.
// `get` supplies the value at drag start; `onDrag` receives the pointer delta
// and that starting value, so clamping stays at the call site.

export function hdrag(opts: {
  get: () => number;
  onDrag: (dx: number, start: number) => void;
}) {
  return (node: HTMLElement) => {
    const down = (e: MouseEvent) => {
      e.preventDefault();
      const start = opts.get();
      const startX = e.clientX;
      const move = (ev: MouseEvent) => opts.onDrag(ev.clientX - startX, start);
      const up = () => {
        window.removeEventListener("mousemove", move);
        window.removeEventListener("mouseup", up);
      };
      window.addEventListener("mousemove", move);
      window.addEventListener("mouseup", up);
    };
    node.addEventListener("mousedown", down);
    return {
      destroy() {
        node.removeEventListener("mousedown", down);
      },
    };
  };
}
