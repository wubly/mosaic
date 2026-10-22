'use client';
import { useEffect, useState } from 'react';
import { motion } from 'framer-motion';
import { MdArrowBack, MdQueueMusic } from 'react-icons/md';
import { usePlayer } from '@/components/player/PlayerProvider';
import { useSpotify } from '@/components/spotify/SpotifyProvider';
import TrackRow from '@/components/tracks/TrackRow';
import TrackTile from '@/components/tracks/TrackTile';
import PlayAll from '@/components/library/PlayAll';
import { gridVariants } from '@/components/tracks/variants';

const compact = (n) => (n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${Math.round(n / 1e3)}K` : String(n || 0));

export function Card({ image, title, sub, round, onClick }) {
  return (
    <motion.div className={`tile ${round ? 'round' : ''}`} whileHover={{ y: -3 }} variants={{ hidden: { opacity: 0, y: 16 }, show: { opacity: 1, y: 0 } }}>
      <button className="tile-main" onClick={onClick}>
        <div className="art" style={image ? { backgroundImage: `url(${image})` } : undefined} />
        <div className="title">{title}</div>
        <div className="artist">{sub}</div>
      </button>
    </motion.div>
  );
}

export const AlbumCards = ({ albums, go }) => (
  <motion.div className="shelf" initial="hidden" animate="show" variants={gridVariants}>
    {albums.map((a) => <Card key={a.id} image={a.image} title={a.name} sub={[a.year, a.kind === 'single' ? 'Single' : a.artist].filter(Boolean).join(' · ')} onClick={() => go({ type: 'album', id: a.id })} />)}
  </motion.div>
);

export const ArtistCards = ({ artists, go }) => (
  <motion.div className="shelf" initial="hidden" animate="show" variants={gridVariants}>
    {artists.map((a) => <Card key={a.id} round image={a.image} title={a.name} sub="Artist" onClick={() => go({ type: 'artist', id: a.id })} />)}
  </motion.div>
);

function useLoad(load, key) {
  const [state, setState] = useState({ key: null, data: null, error: '' });
  useEffect(() => {
    let live = true;
    setState({ key, data: null, error: '' });
    load().then((data) => live && setState({ key, data, error: '' })).catch((e) => live && setState({ key, data: null, error: e.message }));
    return () => { live = false; };
  }, [key]);
  return state.key === key ? state : { data: null, error: '' };
}

function Skeleton() {
  return <div className="shelf">{Array.from({ length: 6 }).map((_, i) => <div key={i} className="skel tile"><i /><b /><b /></div>)}</div>;
}

function Home({ go }) {
  const { data, error } = useLoad(() => window.mosaic.spLibrary(), 'home');
  if (error) return <div className="error">{error}</div>;
  if (!data) return <Skeleton />;
  return (
    <>
      <div className="page-actions left"><PlayAll list={data.saved} /></div>
      {data.saved.length > 0 && (<><h3>Liked songs</h3><div className="shelf">{data.saved.map((t) => <TrackTile key={t.id} t={t} list={data.saved} />)}</div></>)}
      {data.albums.length > 0 && (<><h3>Saved albums</h3><AlbumCards albums={data.albums} go={go} /></>)}
      {data.artists.length > 0 && (<><h3>Followed artists</h3><ArtistCards artists={data.artists} go={go} /></>)}
      {!data.saved.length && !data.albums.length && !data.artists.length && <p className="muted">Nothing saved on Spotify yet. Search for an artist on Discover.</p>}
    </>
  );
}

function Artist({ id, go }) {
  const { data, error } = useLoad(() => window.mosaic.spArtist(id), `artist:${id}`);
  const player = usePlayer();
  if (error) return <div className="error">{error}</div>;
  if (!data) return <Skeleton />;
  const { artist, top, albums } = data;
  return (
    <>
      <div className="pl-head sp-head">
        <div className="sp-cover round" style={artist.image ? { backgroundImage: `url(${artist.image})` } : undefined} />
        <div className="pl-info">
          <p className="muted">Artist</p>
          <h2>{artist.name}</h2>
          <p className="muted">{[artist.followers != null && `${compact(artist.followers)} followers`, artist.genres.slice(0, 3).join(', ')].filter(Boolean).join(' · ')}</p>
          <div className="page-actions left">
            <PlayAll list={top} />
            {top.length > 0 && <button className="tonal" onClick={() => player.enqueueMany(top)}><MdQueueMusic size={20} /> Add to queue</button>}
          </div>
        </div>
      </div>
      {top.length > 0 && (<><h3>Popular</h3><ol className="rows">{top.slice(0, 10).map((t, i) => <TrackRow key={t.id} t={t} i={i} list={top} />)}</ol></>)}
      {albums.length > 0 && (<><h3>Albums & singles</h3><AlbumCards albums={albums} go={go} /></>)}
    </>
  );
}

function Album({ id, go }) {
  const { data, error } = useLoad(() => window.mosaic.spAlbum(id), `album:${id}`);
  const player = usePlayer();
  if (error) return <div className="error">{error}</div>;
  if (!data) return <Skeleton />;
  const { album, tracks } = data;
  return (
    <>
      <div className="pl-head sp-head">
        <div className="sp-cover" style={album.image ? { backgroundImage: `url(${album.image})` } : undefined} />
        <div className="pl-info">
          <p className="muted">{album.kind === 'single' ? 'Single' : 'Album'}</p>
          <h2>{album.name}</h2>
          <p className="muted">
            <a href="#artist" onClick={(e) => { e.preventDefault(); album.artistId && go({ type: 'artist', id: album.artistId }); }}>{album.artist}</a>
            {` · ${album.year} · ${album.total} tracks`}
          </p>
          <div className="page-actions left">
            <PlayAll list={tracks} />
            {tracks.length > 0 && <button className="tonal" onClick={() => player.enqueueMany(tracks)}><MdQueueMusic size={20} /> Add to queue</button>}
          </div>
        </div>
      </div>
      <ol className="rows">{tracks.map((t, i) => <TrackRow key={t.id} t={t} i={i} list={tracks} />)}</ol>
    </>
  );
}

export default function SpotifyView() {
  const { route, stack, go, back } = useSpotify();
  return (
    <div className="library">
      {stack.length > 1 ? (
        <button className="ghost back" onClick={back}><MdArrowBack size={20} /> Back</button>
      ) : (
        <header className="page-head"><div><h2>Spotify <span className="beta-tag">beta</span></h2></div></header>
      )}
      {route.type === 'home' && <Home go={go} />}
      {route.type === 'artist' && <Artist key={route.id} id={route.id} go={go} />}
      {route.type === 'album' && <Album key={route.id} id={route.id} go={go} />}
    </div>
  );
}
