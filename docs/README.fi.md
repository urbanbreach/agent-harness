<div align="center">
  <h1>agent-harness</h1>
  <p>Koodausagentti päätteeseen. Hallitse työkalujen oikeuksia ja palaa tallennettuihin istuntoihin.</p>
  <p>
    <a href="#aloita">Aloita</a> ·
    <a href="#yhdistä-palveluntarjoaja">Yhdistä palveluntarjoaja</a> ·
    <a href="README.md">Dokumentaatio</a> ·
    <a href="../README.md">English</a>
  </p>
</div>

<p align="center">
  <picture>
    <source media="(prefers-reduced-motion: reduce)" srcset="assets/harness-tui.png" />
    <img src="assets/harness-demo.gif" alt="Harnessin päätekäyttöliittymä. Käyttäjä kirjoittaa Hello from PTY, saa testivastauksen Hello world, avaa komentovalikon, hyväksyy testitiedoston muokkauksen ja näkee muutoksen." width="1000" />
  </picture>
</p>

<p align="center">
  Tallenne oikeasta käyttöliittymästä ilman verkkoyhteyttä toimivalla testipalveluntarjoajalla.<br />
  <a href="assets/harness-tui.png">Still-kuva</a> · <a href="assets/README.md">Tallennusohje</a>
</p>

Harness on Rustilla tehty komentorivityökalu, jossa on Ratatui-päätekäyttöliittymä.
Agentit voivat lukea ja muokata tiedostoja, suorittaa komentoja, käyttää määritettyjä
MCP-työkaluja ja delegoida työtä ala-agenteille. Koordinaattori tarkistaa oikeudet
ja ohjaa suoritusta.

Istunto tallentuu tapahtumalokiin, johon lisätään uusia tapahtumia. Voit tarkastella
tai toistaa istunnon suorittamatta työkaluja uudelleen tai ottamatta yhteyttä
palveluntarjoajaan.

## Aloita

Tarvitset Gitin ja vakaan Rust-työkaluketjun, jonka
[`rust-toolchain.toml`](../rust-toolchain.toml) valitsee.

```bash
git clone https://github.com/urbanbreach/agent-harness.git
cd agent-harness
cargo build -p harness --locked

# Kokeile ilman tunnuksia tai verkkoyhteyttä.
./target/debug/harness tui --mock
```

Kirjoita `hello` ja paina Enter. Saat valmiiksi määritellyn testivastauksen.
`Ctrl+p` avaa komentovalikon. Testipalveluntarjoaja vastaa vain testikehotteisiin.
Omia koodaustehtäviä varten tarvitset oikean palveluntarjoajan.

Asenna komento, jos haluat käyttää sitä muissa projekteissa:

```bash
cargo install --path crates/harness --locked
```

## Yhdistä palveluntarjoaja

Kopioi [`configs/harness.example.jsonc`](../configs/harness.example.jsonc)
työstettävän projektin juureen nimellä `harness.jsonc`. Mallina on oletuksena
`openai-codex/gpt-5.4-mini`. Suorita projektin hakemistossa:

```bash
harness config validate
harness doctor
harness auth login codex
harness
```

Aloitusasetukset käyttävät Codexin OAuth-kirjautumista. Varavaihtoehtona on
`OPENAI_API_KEY`. Pidä tunnukset poissa asetustiedostosta. `doctor` tarkistaa
paikalliset asetukset ja tunnusten saatavuuden. Varmista yhteys ja tilin oikeudet
yhdellä oikealla kehotteella.

Harness tukee OpenAI-yhteensopivia ja Anthropic-yhteyksiä.
[Palveluntarjoajien opas](configuration/provider-support.md) kuvaa tunnukset,
mallivalinnan, varamallit ja rajat.

## Käytä Harnessia

| Tehtävä | Komento tai näppäin |
| --- | --- |
| Avaa päätekäyttöliittymä | `harness` |
| Suorita kehote ilman käyttöliittymää | `harness run "Tiivistä tämä projekti"` |
| Avaa komennot ja asetukset | `Ctrl+p` |
| Listaa istunnot | `harness sessions list` |
| Tarkastele istuntoa | `harness sessions inspect <run-id-or-path>` |
| Näytä istuntojen haarat | `harness sessions tree <run-id-or-path>` |
| Selvitä asetuksen lähde | `harness config explain model` |

Pääagentti delegoi työtä `task`-työkalulla. Nimetyt ala-agentit ovat `explore`,
`general` ja `librarian`. Kullakin on omat työkalut ja roolikohtaiset oikeudet.
Projektin yhteiset oikeudet rajoittavat kaikkia ala-agentteja. Lue lisää
[agenteista ja tehtävistä](operations/generic-agent-and-tasks.md).

## Määritä oikeudet

Aloitusasetukset sallivat tavalliset työkalut. Harness kysyy silti luvan projektin
ulkopuolisiin hakemistoihin, toistuviin samanlaisiin kutsuihin ja arkaluonteisten
tiedostojen lukemiseen. Oikeudet säätelevät työkalujen suoritusta. Ne eivät eristä
hyväksyttyä komentoa käyttöjärjestelmän hiekkalaatikkoon.

Tämä `permission`-arvo pyytää luvan muokkauksiin ja sallii vain valitut
komentorivitoiminnot:

```jsonc
"permission": {
  "edit": "ask",
  "bash": {
    "*": "deny",
    "git status*": "allow",
    "cargo nextest run*": "ask"
  },
  "webfetch": "deny"
}
```

Viimeinen täsmäävä sääntö ratkaisee. Kirjoita yleiset säännöt ensin ja poikkeukset
niiden jälkeen. [Oikeusopas](permissions/permissions.md) selittää myös
ala-agenttien rajoitukset.

Ajonaikaiset asetukset kuuluvat tiedostoon `harness.jsonc` ja näppäinasetukset
tiedostoon `tui.jsonc`. `harness config sources` näyttää latausjärjestyksen.
`harness config show --effective` näyttää yhdistetyt asetukset ja peittää salaiset
arvot. [Asetusviite](configuration/config.md) listaa tuetut avaimet.

## Tarkastele ja jaa istunto

```bash
harness sessions inspect <run-id-or-path>
harness sessions export --session-dir <session-dir> --output support-bundle.json <run-id-or-directory-name>
```

Tukipaketti sisältää tapahtumista johdetut metatiedot ja tiedot salaisten arvojen
poistamisesta. Jos tarkistus löytää salaisuuden, vienti ei kirjoita pakettia.
Lue [istunnoista](architecture/sessions-and-replay.md) ja
[paikallisesta datasta](permissions/privacy-and-local-data.md) ennen lokien jakamista.

## Kehitä

Työtila koostuu kuudesta Rust-cratesta. [Arkkitehtuuriohje](architecture/architecture.md)
kuvaa vastuut ja [käyttöliittymän suunnitteluohje](../DESIGN.md) päätteessä toimivat
komponentit.

```bash
cargo fmt --all -- --check
scripts/test-lanes.sh fast
scripts/test-lanes.sh quality-gates
```

Käytä Rust-testeihin nextestiä. [Testausohje](testing/testing.md) kuvaa muut
testiryhmät. Aloita asennusongelmissa [vianmäärityksestä](operations/troubleshooting.md).
