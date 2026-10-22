# mosaic

![mosaic](docs/hero.webp)

a desktop music player for soundcloud and your own files. electron + next.js, material you look.

- play soundcloud (no api key) and local folders in one queue
- word-by-word lyrics from lrclib, with a picker for other versions
- playlists, plus import from spotify links
- equalizer, sleep timer, mini player, discord rich presence
- colors follow the album art
- beta (settings > beta): spotify and youtube playback, artist and album pages, genius lyrics

<table>
  <tr>
    <td><img src="docs/screenshots/lyrics.webp" alt="lyrics"></td>
    <td><img src="docs/screenshots/now-playing.webp" alt="now playing"></td>
  </tr>
</table>

## run it

```
npm install
npm run dev
```

`npm run dist` builds a windows installer, `npm run dist:mac` a dmg. press ctrl+k in the app for everything else.

## notes

- soundcloud access is unofficial and can break. personal use only.
- not affiliated with soundcloud, spotify, discord or lrclib.

## more notes
- dont bug report, jsut make pr :3
- works on windows 10 and macOS
- beta features (settings > beta): spotify, youtube, genius lyrics. spotify needs premium and your own client id, youtube needs your own google oauth client. spotify playback needs widevine, so mosaic ships on castlabs electron. to play spotify from source on mac sign it once: `python3 -m castlabs_evs.vmp sign-pkg node_modules/electron/dist` 
