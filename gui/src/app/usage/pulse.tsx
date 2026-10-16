import { useEffect, useState, type ReactNode } from 'react';
import type { StyleDesc } from '@gpuix/react';
import { useTheme } from '../../theme';

export function Pulse({ children, style }: { children: ReactNode; style?: StyleDesc }) {
  const { reducedMotion } = useTheme();
  const [bright, setBright] = useState(false);
  useEffect(() => {
    if (reducedMotion) return;
    const timer = setInterval(() => setBright((value) => !value), 700);
    return () => clearInterval(timer);
  }, [reducedMotion]);
  return (
    <div
      motion={{
        initial: false,
        animate: { opacity: reducedMotion ? 0.9 : bright ? 0.9 : 0.45 },
        transition: { duration: reducedMotion ? 0 : 0.7, ease: 'easeInOut' },
      }}
      style={style}
    >
      {children}
    </div>
  );
}
