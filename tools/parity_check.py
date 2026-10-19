"""Paridad Smart Mix vs MIK 11 — emparejando por (artist, title)."""
import json
import sqlite3
import re
import unicodedata

MIK_DB = r"C:\Users\Crist\AppData\Local\Mixed In Key\Mixed In Key\11.0\MIKStore.db"
RESULTS = r"C:\Users\Crist\Dev\smart-mix-desktop\batch_results.json"


def norm(s):
    s = unicodedata.normalize("NFKD", str(s)).lower()
    return re.sub(r"[^a-z0-9]+", " ", s).strip()


def parse_smart_mix_name(fname):
    """'A_Minor_-_120_-_Title_-_Artist.aiff' → (title, artist)
       o '1 - Artist - Title (Mix).mp3' → (title, artist)."""
    stem = re.sub(r"\.(aiff|aif|mp3|wav|flac|m4a)$", "", fname, flags=re.I)
    m = re.match(r"^[A-G](#|b)?_?(Major|Minor)_-_(\d+)_-_(.+)_-_(.+)$", stem)
    if m:
        return norm(m.group(4)), norm(m.group(5))
    m = re.match(r"^\d+\s*-\s*(.+?)\s*-\s*(.+)$", stem)
    if m:
        return norm(m.group(2)), norm(m.group(1))
    return norm(stem), None


def parse_key(k):
    m = re.match(r"^(\d{1,2})([AB])$", str(k).strip().upper())
    return (int(m.group(1)), m.group(2)) if m else None


def main():
    results = [r for r in json.load(open(RESULTS)) if not r.get("error")]
    con = sqlite3.connect(f"file:{MIK_DB}?mode=ro", uri=True)
    con.row_factory = sqlite3.Row
    rows = con.execute("SELECT SongName, ArtistName, Tempo, OverallEnergy, MainKey FROM Song").fetchall()
    mik = {}
    for r in rows:
        mik[(norm(r["SongName"]), norm(r["ArtistName"]))] = r
    key_exact = key_equiv = bpm_match = e_exact = e_pm1 = n = 0
    mismatches = []
    for r in results:
        title, artist = parse_smart_mix_name(r["file"])
        if not artist:
            continue
        m = mik.get((title, artist))
        if not m:
            continue
        n += 1
        mk, mt, me = m["MainKey"], float(m["Tempo"] or 0), m["OverallEnergy"]
        sk, sb, se = r.get("key"), r.get("bpm"), r.get("energy")
        if mk and sk:
            if sk == mk:
                key_exact += 1
            else:
                a, b = parse_key(sk), parse_key(mk)
                if a and b:
                    d = abs(a[0] - b[0])
                    if (a[1] == b[1] and d in (1, 11)) or (a[0] == b[0]):
                        key_equiv += 1
                    else:
                        mismatches.append((r["file"][:44], sk, mk))
        if mt and sb and sb > 0 and abs(mt - sb) <= 0.35:
            bpm_match += 1
        if me is not None and se is not None:
            if abs(float(me) - se) < 0.5:
                e_exact += 1
            if abs(float(me) - se) <= 1.5:
                e_pm1 += 1
    print(f"=== PARIDAD Smart Mix vs MIK 11 (n={n}) ===")
    if n:
        print(f"Key exacta         : {key_exact}/{n} = {key_exact/n*100:.1f}%")
        print(f"Key exacta+equival.: {key_exact+key_equiv}/{n} = {(key_exact+key_equiv)/n*100:.1f}%")
        print(f"BPM ±0.35          : {bpm_match}/{n} = {bpm_match/n*100:.1f}%")
        print(f"Energy ±0.5        : {e_exact}/{n} = {e_exact/n*100:.1f}%")
        print(f"Energy ±1.5        : {e_pm1}/{n} = {e_pm1/n*100:.1f}%")
        print(f"mismatches key: {len(mismatches)}")
        for f, sk, mk in mismatches[:15]:
            print(f"  {f:46} SM={sk:4} MIK={mk}")
    con.close()


if __name__ == "__main__":
    main()
