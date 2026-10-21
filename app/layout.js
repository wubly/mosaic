import './globals.css';

export const metadata = { title: 'Mosaic', description: 'A SoundCloud music player' };

export default function RootLayout({ children }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <body>{children}</body>
    </html>
  );
}
