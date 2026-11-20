"""Made-up data shaped like Sam's (persona study): green-taxi trips over three
months with the NULLs, refunds and stray dates real trip files have, a zone
lookup of 263 zones in six boroughs, and a small music store with the
Chinook schema (artists, albums, tracks, invoices, a management chain).

Nothing is copied from a real data set: every name, number and date comes from
the seeded generator below, so the battery's answers differ from round 4's but
are the same on every machine.

    python3 gen.py DIR [TRIPS]     # DIR/trips.parquet, DIR/zones.parquet, DIR/music.sqlite
"""

import csv
import datetime
import os
import random
import sqlite3
import sys

import duckdb

BOROUGHS = [("Manhattan", 69, "Yellow Zone"), ("Brooklyn", 61, "Boro Zone"), ("Queens", 69, "Boro Zone"),
            ("Bronx", 43, "Boro Zone"), ("Staten Island", 20, "Boro Zone"), ("EWR", 1, "EWR")]
PARTS = ["Mill", "Harbor", "Park", "Heights", "Village", "Gardens", "Point", "Hill", "Bay", "Grove", "Square", "Ridge",
         "Meadow", "Falls", "Bridge", "Landing", "Commons", "Terrace", "Crossing", "Yards", "Junction", "Hollow"]
LEADS = ["East", "West", "North", "South", "Upper", "Lower", "Old", "New", "", "", "", ""]


def zones(rnd):
    out, used, i = [], set(), 1
    for borough, n, service in BOROUGHS:
        for _ in range(n):
            while True:
                name = " ".join(x for x in [rnd.choice(LEADS), rnd.choice(PARTS), rnd.choice(PARTS)] if x)
                if rnd.random() < 0.08:
                    name += "/" + rnd.choice(PARTS)
                if name not in used:
                    used.add(name)
                    break
            sz = service if rnd.random() > 0.03 else None
            out.append((i, name, borough, sz))
            i += 1
    return out


def trips(rnd, n, out_csv):
    # pickups: a few zones carry most of them (74 the most), twenty never have one
    ids = list(range(1, 264))
    w = {z: 1.0 / (1 + (k % 97)) for k, z in enumerate(ids)}
    for z, share in {74: 0.19, 75: 0.12, 41: 0.06, 166: 0.04, 116: 0.011, 196: 0.005, 33: 0.004}.items():
        w[z] = share
    never = set(rnd.sample([z for z in ids if z not in (74, 75, 41, 166, 116, 196, 33)], 20))
    rest = 1 - sum(v for z, v in w.items() if z in (74, 75, 41, 166, 116, 196, 33))
    others = [z for z in ids if z not in (74, 75, 41, 166, 116, 196, 33) and z not in never]
    total_other = sum(w[z] for z in others)
    pu_w = [w[z] if z in (74, 75, 41, 166, 116, 196, 33) else (w[z] / total_other * rest if z in others else 0) for z in ids]
    do_w = [0.5 * a + 0.5 / 263 for a in pu_w]
    start = datetime.datetime(2023, 1, 1)
    span = (datetime.datetime(2023, 4, 1) - start).total_seconds()
    with open(out_csv, "w", newline="") as f:
        wr = csv.writer(f)
        wr.writerow(["trip_id", "VendorID", "lpep_pickup_datetime", "lpep_dropoff_datetime", "store_and_fwd_flag", "PULocationID",
                     "DOLocationID", "passenger_count", "trip_distance", "fare_amount", "tip_amount", "total_amount", "payment_type"])
        pus = rnd.choices(ids, weights=pu_w, k=n)
        dos = rnd.choices(ids, weights=do_w, k=n)
        for i in range(n):
            r = rnd.random()
            if r < 0.0004:
                pu = datetime.datetime(rnd.choice([2009, 2022, 2023]), rnd.choice([1, 12, 4, 6]), rnd.randint(1, 28), rnd.randint(0, 23), rnd.randint(0, 59))
            else:
                pu = start + datetime.timedelta(seconds=int(rnd.random() * span))
            dist = round(rnd.expovariate(1 / 2.8), 2) if rnd.random() > 0.02 else None
            mins = max(1, int((dist or 1) * rnd.uniform(2.5, 6))) if rnd.random() > 0.001 else rnd.randint(500, 1500)
            do = pu + datetime.timedelta(minutes=mins, seconds=rnd.randint(0, 59))
            pay = rnd.choices([1, 2, 3, 4, 5, None], weights=[60, 30, 1, 1, 0.2, 8])[0]
            fare = round(3 + (dist or 1) * 2.5 + rnd.uniform(-1, 4), 2)
            if rnd.random() < 0.003:
                fare = -fare
            tip = round(fare * rnd.choice([0, 0.1, 0.15, 0.2, 0.25]), 2) if pay == 1 and fare > 0 else 0.0
            total = round(fare + tip + rnd.choice([1.0, 1.5, 2.75, 3.25]), 2)
            pax = rnd.choices([1, 2, 3, 4, 5, 6, 0, None], weights=[70, 12, 3, 2, 3, 2, 1, 7])[0]
            wr.writerow([i + 1, rnd.choice([1, 2, 2]), pu.isoformat(sep=" "), do.isoformat(sep=" "), rnd.choices(["N", "Y", ""], weights=[95, 1, 4])[0],
                         pus[i], dos[i], "" if pax is None else pax, "" if dist is None else dist, fare, tip, total, "" if pay is None else pay])


WORDS = ["Love", "Night", "Road", "Fire", "River", "Heart", "Dream", "Light", "Stone", "Wild", "Blue", "Gold", "Rain",
         "Song", "Ghost", "Summer", "City", "Dance", "Echo", "Storm", "Glass", "Silver", "Home", "Moon", "Rose", "Time"]
FIRST = ["Ana", "Bo", "Cai", "Dara", "Eli", "Femi", "Gus", "Hana", "Ivo", "Jun", "Kai", "Lior", "Mina", "Noor", "Oren",
         "Pia", "Quin", "Rui", "Sol", "Tae", "Uma", "Vik", "Wen", "Xan", "Yara", "Zev"]
LAST = ["Abara", "Brenn", "Castell", "Dorn", "Elm", "Frey", "Gale", "Holt", "Ide", "Joss", "Kade", "Lund", "Marr", "Nyx",
        "Orme", "Pell", "Quist", "Rook", "Sand", "Tarn", "Ulm", "Vale", "Wren", "Yost", "Zane"]
COUNTRIES = ["Brazil", "Canada", "Chile", "Denmark", "France", "Germany", "India", "Ireland", "Italy", "Japan", "Kenya",
             "Mexico", "Norway", "Poland", "Portugal", "Spain", "Sweden", "USA", "United Kingdom", "Uruguay"]


def title(rnd, n):
    return " ".join(rnd.choice(WORDS) for _ in range(n))


def music(rnd, path):
    if os.path.exists(path):
        os.remove(path)
    db = sqlite3.connect(path)
    c = db.cursor()
    c.executescript("""
    CREATE TABLE Genre (GenreId INTEGER PRIMARY KEY, Name TEXT);
    CREATE TABLE MediaType (MediaTypeId INTEGER PRIMARY KEY, Name TEXT);
    CREATE TABLE Artist (ArtistId INTEGER PRIMARY KEY, Name TEXT);
    CREATE TABLE Album (AlbumId INTEGER PRIMARY KEY, Title TEXT, ArtistId INTEGER);
    CREATE TABLE Track (TrackId INTEGER PRIMARY KEY, Name TEXT, AlbumId INTEGER, MediaTypeId INTEGER, GenreId INTEGER,
                        Composer TEXT, Milliseconds INTEGER, Bytes INTEGER, UnitPrice REAL);
    CREATE TABLE Playlist (PlaylistId INTEGER PRIMARY KEY, Name TEXT);
    CREATE TABLE PlaylistTrack (PlaylistId INTEGER, TrackId INTEGER, PRIMARY KEY (PlaylistId, TrackId));
    CREATE TABLE Employee (EmployeeId INTEGER PRIMARY KEY, LastName TEXT, FirstName TEXT, ReportsTo INTEGER,
                           BirthDate DATETIME, HireDate DATETIME);
    CREATE TABLE Customer (CustomerId INTEGER PRIMARY KEY, FirstName TEXT, LastName TEXT, Country TEXT, SupportRepId INTEGER);
    CREATE TABLE Invoice (InvoiceId INTEGER PRIMARY KEY, CustomerId INTEGER, InvoiceDate DATETIME, BillingCountry TEXT, Total REAL);
    CREATE TABLE InvoiceLine (InvoiceLineId INTEGER PRIMARY KEY, InvoiceId INTEGER, TrackId INTEGER, UnitPrice REAL, Quantity INTEGER);
    """)
    genres = ["Rock", "Jazz", "Metal", "Alternative", "Blues", "Latin", "Reggae", "Pop", "Soundtrack", "Classical", "Folk",
              "Electronic", "Hip Hop", "Soul", "Country", "World", "Ambient", "Punk", "Funk", "Gospel", "Opera", "Ska", "Drama", "Comedy", "Indie"]
    c.executemany("INSERT INTO Genre VALUES (?, ?)", list(enumerate(genres, 1)))
    c.executemany("INSERT INTO MediaType VALUES (?, ?)", list(enumerate(["MPEG audio file", "Protected AAC audio file", "Protected MPEG-4 video file",
                                                                         "Purchased AAC audio file", "AAC audio file"], 1)))
    artists = [(i, f"The {title(rnd, 1)} {rnd.choice(['Band', 'Trio', 'Collective', 'Orchestra', 'Project'])}" if rnd.random() < 0.5
                else f"{rnd.choice(FIRST)} {rnd.choice(LAST)}") for i in range(1, 276)]
    c.executemany("INSERT INTO Artist VALUES (?, ?)", artists)
    albums = [(i, title(rnd, rnd.randint(1, 3)), rnd.choices(range(1, 276), weights=[1 / (1 + (k % 40)) for k in range(275)])[0]) for i in range(1, 348)]
    c.executemany("INSERT INTO Album VALUES (?, ?, ?)", albums)
    tracks = []
    for i in range(1, 3504):
        g = rnd.choices(range(1, 26), weights=[40, 4, 11, 12, 2, 16, 2, 14, 4, 2] + [1] * 15)[0]
        comp = None if rnd.random() < 0.28 else f"{rnd.choice(FIRST)} {rnd.choice(LAST)}"
        ms = int(rnd.lognormvariate(12.3, 0.4))
        mt = rnd.choices(range(1, 6), weights=[85, 7, 6, 1, 1])[0]
        price = 1.99 if mt == 3 else 0.99
        tracks.append((i, title(rnd, rnd.randint(1, 4)), rnd.randint(1, 347), mt, g, comp, ms, ms * 32 + rnd.randint(0, 9999), price))
    c.executemany("INSERT INTO Track VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)", tracks)
    c.executemany("INSERT INTO Playlist VALUES (?, ?)", [(i, f"{title(rnd, 2)} Mix") for i in range(1, 19)])
    pt = set()
    while len(pt) < 8715:
        pt.add((rnd.choices(range(1, 19), weights=[30, 1, 1, 1, 30, 1, 1, 30, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2])[0], rnd.randint(1, 3503)))
    c.executemany("INSERT INTO PlaylistTrack VALUES (?, ?)", sorted(pt))
    emps = [(1, "Abara", "Ana", None, "1962-02-18 00:00:00", "2002-08-14 00:00:00")]
    for i, boss in zip(range(2, 9), [1, 2, 2, 2, 1, 6, 6]):
        emps.append((i, rnd.choice(LAST), rnd.choice(FIRST), boss, f"19{rnd.randint(58, 79)}-{rnd.randint(1, 12):02d}-{rnd.randint(1, 28):02d} 00:00:00",
                     f"200{rnd.randint(2, 4)}-{rnd.randint(1, 12):02d}-{rnd.randint(1, 28):02d} 00:00:00"))
    c.executemany("INSERT INTO Employee VALUES (?, ?, ?, ?, ?, ?)", emps)
    custs = [(i, rnd.choice(FIRST), rnd.choice(LAST) + ("" if rnd.random() < 0.8 else "e"), rnd.choices(COUNTRIES, weights=[3, 4, 1, 1, 4, 3, 2, 1, 1, 1, 1, 1, 1, 1, 2, 1, 1, 9, 3, 1])[0],
              rnd.choice([3, 4, 5])) for i in range(1, 60)]
    c.executemany("INSERT INTO Customer VALUES (?, ?, ?, ?, ?)", custs)
    inv, lines, line_id = [], [], 1
    day = datetime.datetime(2021, 1, 1)
    for i in range(1, 413):
        cust = custs[(i * 7) % 59]
        when = day + datetime.timedelta(days=int(i * 1826 / 412))
        total = 0.0
        for _ in range(rnd.choice([1, 2, 2, 4, 6, 9, 14])):
            t = rnd.randint(1, 3503)
            price = tracks[t - 1][8]
            lines.append((line_id, i, t, price, 1))
            line_id += 1
            total += price
        inv.append((i, cust[0], when.isoformat(sep=" "), cust[3], round(total, 2)))
    c.executemany("INSERT INTO Invoice VALUES (?, ?, ?, ?, ?)", inv)
    c.executemany("INSERT INTO InvoiceLine VALUES (?, ?, ?, ?, ?)", lines)
    db.commit()
    db.close()


def main():
    out = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 205064
    os.makedirs(out, exist_ok=True)
    rnd = random.Random(42)
    zs = zones(rnd)
    con = duckdb.connect()
    con.execute("CREATE TABLE zones (LocationID BIGINT, Zone VARCHAR, Borough VARCHAR, service_zone VARCHAR)")
    con.executemany("INSERT INTO zones VALUES (?, ?, ?, ?)", zs)
    con.execute(f"COPY zones TO '{out}/zones.parquet' (FORMAT parquet)")
    trips(random.Random(43), n, os.path.join(out, "trips.csv"))
    con.execute(f"""COPY (SELECT * FROM read_csv('{out}/trips.csv', header = true, nullstr = '', columns = {{
        'trip_id': 'BIGINT', 'VendorID': 'BIGINT', 'lpep_pickup_datetime': 'TIMESTAMP', 'lpep_dropoff_datetime': 'TIMESTAMP',
        'store_and_fwd_flag': 'VARCHAR', 'PULocationID': 'BIGINT', 'DOLocationID': 'BIGINT', 'passenger_count': 'BIGINT',
        'trip_distance': 'DOUBLE', 'fare_amount': 'DOUBLE', 'tip_amount': 'DOUBLE', 'total_amount': 'DOUBLE', 'payment_type': 'BIGINT'}}))
        TO '{out}/trips.parquet' (FORMAT parquet)""")
    os.remove(os.path.join(out, "trips.csv"))
    music(random.Random(44), os.path.join(out, "music.sqlite"))
    print(out, n, "trips")


if __name__ == "__main__":
    main()
