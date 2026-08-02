# Arkkitehtuuri ja itseään rakentava palaute

Kun projekti kasvoi, tärkeimmäksi arkkitehtuuriratkaisuksi nousi vastuiden pitäminen selkeinä. Käyttöliittymä saa näyttää ja välittää pyyntöjä, mutta se ei saa päättää ajon totuudesta. Työkalut saavat suorittaa rajattuja toimintoja, mutta eivät ohittaa käyttöoikeuksia. Tapahtumaloki puolestaan säilyttää sen, mitä todella tapahtui.

## Rust-työtilan rakenne

Koodi on jaettu kuuteen crateen. Jaon tarkoitus ei ole vain siisti hakemistorakenne, vaan muutosten vaikutusalueen rajaaminen. Esimerkiksi TUI voi muuttua ilman, että tapahtumamallin perussäännöt siirtyvät käyttöliittymäkerrokseen. Kaavio kokoaa vastuualueet lukujärjestykseen. Se ei kuvaa cratejen välisiä riippuvuuksia. Kaavion alla oleva taulukko kertoo kunkin craten varsinaisen vastuun.

```mermaid
flowchart TD
    H["Käynnistys<br/>harness"]
    U["Käyttöliittymä<br/>harness-tui"]
    C["Ajonaikaiset säännöt<br/>harness-core"]
    T["Työkaluraja<br/>harness-tools"]
    P["Palveluntarjoajaraja<br/>harness-providers"]
    K["Testituki<br/>harness-testkit"]
    H ~~~ U
    U ~~~ C
    C ~~~ T
    T ~~~ P
    P ~~~ K
```

| Crate | Vastuu ja tärkein oppi |
| --- | --- |
| `harness` | Omistaa komentorivin, sovelluksen käynnistyksen ja muiden cratejen kokoamisen. Käynnistyskerroksen kannattaa pysyä ohuena, jotta ajonaikaiset säännöt eivät hajaannu siihen. |
| `harness-core` | Omistaa koordinaation, tapahtumat, käyttöoikeudet, replayn ja elinkaaren. Yhdellä asialla pitää olla yksi selkeä päätöksentekijä. |
| `harness-providers` | Rajaa palveluntarjoajien suoratoiston ja redaktoidun metatiedon. Ulkoisen rajapinnan yksityiskohdat eivät saa vuotaa koko järjestelmään. |
| `harness-tools` | Omistaa työkalujen skeemat, polkuturvan, komentotulkin ja integraatiot. Työkalun nimi, syöte ja käyttöoikeus muodostavat yhden sopimuksen. |
| `harness-tui` | Omistaa näkymät, dialogit ja näppäimistökäytön. TUI on tilakone, ei vain tekstin piirtämistä. |
| `harness-testkit` | Tarjoaa deterministiset testikaksoisolennot, simulaatiot ja pääteajojen apurit. Testinäyttö tarvitsee toistettavan ympäristön. |

## Koordinaattori ajon keskuksena

Koordinaattori käsittelee komennot sarjassa. Se päättää tehtävien ajoituksesta, käyttöoikeuksista, tapahtumien lisäämisestä, kompaktioinnista ja elinkaaren muutoksista. Näin usea asynkroninen toiminto ei pääse kirjoittamaan keskenään ristiriitaista tilaa.

```mermaid
flowchart TD
    U[Käyttäjä] --> S[CLI tai TUI]
    S --> C[Koordinaattori]
    C --> P[Käyttöoikeuspäätös]
    P --> X[Työkalu tai palveluntarjoaja]
    X --> E[JSONL-tapahtuma]
    E --> R[Replay ja näkyvä tila]
```

Yhden työkalukutsun aikana tapahtuu enemmän kuin käyttöliittymästä näkyy. Ensin tarkistetaan oikeus, sitten työkalu suoritetaan ja vasta lopuksi tulos kirjataan tapahtumaksi. Jos käyttäjän päätös tarvitaan, ajo jää odottamaan juuri siinä kohdassa.

```mermaid
flowchart TD
    K[Käyttäjän pyyntö] --> C[Koordinaattori]
    C --> O[Käyttöoikeuden ratkaisu]
    O --> T[Työkalun suoritus]
    T --> E[Tulostapahtuma lokiin]
    E --> U[Näkyvän tilan päivitys]
```

## Tapahtumaloki ja replay

JSONL-loki on järjestelmän pysyvä muisti. Replay lukee tapahtumat järjestysnumeroiden mukaisesti ja johtaa niistä sessiot, tehtävät, keskusteluhistorian ja muut näkymät. Se ei käynnistä palveluntarjoajaa, työkaluja tai tapahtumakoukkuja uudelleen. Tämä raja tekee vanhan ajon tutkimisesta turvallista.

```mermaid
flowchart TD
    J[events.jsonl] --> S[Järjestysnumeroiden tarkistus]
    S --> P[Projektiot]
    P --> N[Sessiot, tehtävät ja keskusteluhistoria]
    N --> I[Inspect ja replay]
    I --> X[Ei verkkokutsuja eikä työkaluajoja]
```

## Pää- ja alisessiot

Alisessio ei ole irrallinen prosessi, vaan osa samaa tapahtumaketjua. Koordinaattori luo lapsisession, rajaa sen kontekstin ja oikeudet sekä peilaa olennaiset tapahtumat takaisin pääsessioon. Jos peruutettu tehtävä valmistuu myöhässä, tulos kirjataan myöhäiseksi eikä sitä enää päästetä muuttamaan työtilaa.

```mermaid
flowchart TD
    P[Pääsessio] --> Q[task-työkalu]
    Q --> C[Koordinaattori]
    C --> L[Lapsisessio]
    L --> R[Tulos tai virhe]
    R --> N[Pääsessioon peilattu tapahtuma]
```

## Järjestelmä oman kehityksen tukena

Projektin kiinnostavin vaihe alkoi, kun järjestelmästä tuli riittävän toimiva oman koodinsa tutkimiseen ja muuttamiseen. Tällöin kehitystyö alkoi ruokkia itseään. Työkalut nopeuttivat seuraavaa muutosta, mutta jokainen uusi kyky vaati samalla tarkemmat käyttöoikeudet ja paremman testinäytön.

```mermaid
flowchart TD
    A[Muutosidea] --> B[Agenttityökalut]
    B --> C[Muutos ja testi]
    C --> D[Tapahtumat ja näyttö]
    D --> E[Palautettu havainto]
```

Olisin hyötynyt yhdenmukaisesta testikuitista jo projektin alussa. Silloin jokainen merkittävä muutos olisi saanut heti omistajan, testin, tulospolun ja selkeän rajauksen.
