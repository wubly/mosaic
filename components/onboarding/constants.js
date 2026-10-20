export const STEPS = 5;

export const slide = {
  initial: { opacity: 0, x: 28 },
  animate: { opacity: 1, x: 0, transition: { type: 'spring', stiffness: 280, damping: 30 } },
  exit: { opacity: 0, x: -28, transition: { duration: 0.14 } },
};

export const baseName = (path) => path.split(/[\\/]/).filter(Boolean).pop() || path;

export const newId = () => Math.random().toString(36).slice(2, 10);
