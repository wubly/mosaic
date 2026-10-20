'use client';

export default function Resizer({ className, label, onDrag, onCommit, onReset }) {
  const start = (e) => {
    e.currentTarget.setPointerCapture(e.pointerId);
    e.currentTarget.dataset.drag = '1';
  };

  const end = (e) => {
    delete e.currentTarget.dataset.drag;
    e.currentTarget.releasePointerCapture(e.pointerId);
    onCommit();
  };

  return (
    <div
      className={`rz ${className}`}
      role="separator"
      aria-label={label}
      onPointerDown={start}
      onPointerMove={(e) => e.currentTarget.dataset.drag && onDrag(e)}
      onPointerUp={end}
      onDoubleClick={onReset}
    />
  );
}
