export default function Bars({ playing }) {
  return (
    <span className={`eqbars ${playing ? 'on' : ''}`} aria-hidden>
      <i /><i /><i />
    </span>
  );
}
