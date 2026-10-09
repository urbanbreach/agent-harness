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

Harness toimii vain Linuxissa (x86_64 ja aarch64). macOS:ää ja Windowsia ei
tueta eikä testata.

Asenna uusin julkaisu:

```bash
curl -fsSL https://github.com/urbanbreach/agent-harness/releases/latest/download/install.sh | sh
cd /polku/projektiisi
harness
```

Skripti lataa prosessorillesi sopivan staattisen binäärin, tarkistaa sen
julkaisun `SHA256SUMS`-tiedostoa vasten ja asentaa sen hakemistoon
`~/.local/bin`. `HARNESS_VERSION=v0.1.0` valitsee tietyn julkaisun ja
`HARNESS_INSTALL_DIR` toisen asennushakemiston. Poista asennus poistamalla
`harness`-tiedosto.

Asetustiedostoa ei tarvita. Kun projektissa ei ole yhdistettyä palveluntarjoajaa
eikä tallennettuja istuntoja, Harness avaa kirjautumisvalikon automaattisesti.
Valitse palveluntarjoaja ja kirjaudu. Kirjoita sitten koodaustehtävä ja paina
Enter. Esc sulkee valikon; `/login` avaa sen uudelleen. Jos projektissa on jo
istuntoja mutta yhteys puuttuu, aloitusnäkymässä lukee
`No provider connected. Use /login.`

Voit kokeilla käyttöliittymää ilman verkkoa komennolla `harness tui --mock`.
Kirjoita `hello` ja paina Enter, niin saat testivastauksen. Testipalveluntarjoaja
vastaa vain testikehotteisiin. `Ctrl+p` avaa komentovalikon.

### Käännä lähdekoodista

Tarvitset Gitin ja vakaan Rust-työkaluketjun, jonka
[`rust-toolchain.toml`](../rust-toolchain.toml) valitsee. Muita linkkereitä tai
työkaluja ei tarvita.

```bash
git clone https://github.com/urbanbreach/agent-harness.git
cd agent-harness
cargo install --path crates/harness --locked
```

## Yhdistä palveluntarjoaja

Käyttöliittymän `/login` avaa palveluntarjoajavalikon. Vaihtoehtoina ovat
OpenAI ChatGPT Plus/Pro tai API-avain, GitHub Copilotin laitekirjautuminen,
Anthropicin API-avain, Claude Pro/Max -tilaus, Google, OpenRouter ja muita
palveluntarjoajia.

Voit kirjautua myös komentoriviltä:

```bash
harness auth login codex
# Muut palveluntarjoajat: harness auth login <provider>
harness doctor
harness
```

Ympäristömuuttujan API-avain riittää ilman kirjautumiskomentoa tai asetustiedostoa:

```bash
export ANTHROPIC_API_KEY="oma-api-avain"
harness
```

Harness löytää tallennetut tunnukset ja palveluntarjoajien API-avainten
ympäristömuuttujat sisäänrakennetun models.dev-luettelon perusteella. Pidä
tunnukset poissa asetustiedostoista. `harness doctor` tarkistaa valmiuden ilman
verkkoa.
Oikea kehote varmistaa tilin ja palveluntarjoajan yhteyden.
[Palveluntarjoajien opas](configuration/provider-support.md) kertoo lisää.

Asetustiedosto on valinnainen. Ensimmäisen onnistuneen kirjautumisen jälkeen
Harness luo tiedoston `~/.harness/harness.jsonc` ja valitsee siihen kyseisen
palveluntarjoajan oletusmallin, jos malli on tiedossa. Harness ei koskaan
korvaa olemassa olevaa käyttäjän asetustiedostoa. Se ei myöskään kirjoita
tiedostoa, jos `HARNESS_CONFIG` tai `HARNESS_CONFIG_CONTENT` on asetettu.
Tallenna omat oletuksesi tähän tiedostoon ja projektin säännöt tiedostoon
`<projekti>/harness.jsonc`. Projektin asetukset ohittavat omat oletuksesi.
`HARNESS_HOME` vaihtaa käyttäjän asetushakemiston.
[`configs/harness.example.jsonc`](../configs/harness.example.jsonc) on lyhyt,
kommentoitu esimerkki. Se ei valitse mallia eikä sitä tarvitse kopioida.

Jos asetuksissa ei ole palveluntarjoajia, automaattinen haku pysyy käytössä.
Yksikin `provider`-määritys rajaa luettelon määritettyihin palveluntarjoajiin
sekä kirjautuneisiin Codex-, GitHub Copilot- ja Claude-tilaustileihin.

## Käytä Harnessia

| Tehtävä | Komento tai näppäin |
| --- | --- |
| Avaa päätekäyttöliittymä | `harness` |
| Suorita kehote ilman käyttöliittymää | `harness run "Tiivistä tämä projekti"` |
| Avaa komennot ja asetukset | `Ctrl+p` |
| Listaa istunnot | `harness sessions list` |
| Tarkastele istuntoa | `harness sessions inspect <run-id-or-path>` |
| Näytä istuntojen haarat | `harness sessions tree --root <run-id-or-path>` |
| Selvitä asetuksen lähde | `harness config explain model` |

Pääagentti delegoi työtä `spawn_subagent`-työkalulla. Valmiit ala-agentit ovat
`task`, `scout`, `reviewer`, `security-reviewer` ja `sonic`. Lisää omat
Markdown-määritykset YAML-alkutietoineen hakemistoon
`<projekti>/.harness/agents/` tai `<home>/agents/`. Lähin projektin määritys
voittaa käyttäjän määrityksen, joka puolestaan voittaa valmiin agentin.
`task` perii pääagentin mallin. Scout ja sonic käyttävät valitsinta `@smol`,
arvioijat valitsinta `@slow`. Aseta nämä valinnaiset roolit avaimella
`model_roles`; määrittämätön rooli perii pääagentin mallin. Projektin yhteiset
oikeudet rajoittavat kaikkia ala-agentteja. Lue lisää
[agenteista ja tehtävistä](operations/generic-agent-and-tasks.md).

Omat vinoviivakomennot ovat Markdown-kehotteita hakemistossa
`<projekti>/.harness/commands/` tai `<home>/commands/`. Ne toimivat TUI:ssa sekä
komennoilla `harness run "/nimi argumentit"` ja `harness prompt`. Valmis
`/init` pyytää agenttia luomaan tai päivittämään tiiviin `AGENTS.md`-tiedoston
projektin juureen käyttäjän sisältöä säilyttäen. Katso
[komentopohjat](operations/extension-strategy.md#markdown-prompt-commands).

## Määritä oikeudet

Oletusoikeudet sallivat tavalliset työkalut. Harness kysyy silti luvan projektin
ulkopuolisiin hakemistoihin, toistuviin samanlaisiin kutsuihin ja arkaluonteisten
tiedostojen lukemiseen. Oikeudet säätelevät työkalujen suoritusta. Ne eivät eristä
hyväksyttyä komentoa käyttöjärjestelmän hiekkalaatikkoon.

Lisää tämä lohko valinnaiseen omaan tai projektin asetustiedostoon, jos haluat
hyväksyä muokkaukset ja sallia vain valitut komentorivitoiminnot:

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

## Missä data on

Käyttäjän tiedostot ovat hakemistossa `~/.harness/`. Jos `HARNESS_HOME` ei ole
tyhjä, Harness käyttää sen arvoa sellaisenaan eikä lisää polkuun hakemistoa.
Hakemiston sisältö:

- `harness.jsonc` tai `harness.json`: käyttäjän ajonaikaiset asetukset.
  Ensimmäinen olemassa oleva tiedosto valitaan.
- `tui.jsonc` tai `tui.json`: käyttäjän näppäinasetukset samalla valintasäännöllä.
- `credentials/`, `anthropic-subscription-bindings/`, `models-cache.json`:
  tallennetut tunnukset, tilaustilien sidokset ja malliluettelon välimuisti.
- `prompts/`, `agents/`, `commands/`, `skills/`: käyttäjän kehotepohjat,
  agentit, vinoviivakomennot ja taidot.
- `model.json`: viimeksi TUI:ssa valittu malli.
  `HARNESS_MODEL_SELECTION_STATE_FILE` voi vaihtaa tiedoston polun.
- `sessions/<key>/`: istunnot ja niiden tiedostot.
- `projects/<key>/`: muisti, koodi-indeksi, muokkausten jäljitys ja suunnitelmat.
- `worktrees/<key>/`: hallitut Git-työpuut.

Globaalit taidot latautuvat oletuksena hakemistoista `<home>/skills` ja
`$HOME/.agents/skills`. `HARNESS_HOME` vaikuttaa vain ensimmäiseen polkuun.
Vaihda `skills.global_roots`, jos säilytät taitoja muualla. Projektin avain
muodostuu sen polusta. Projektin `/home/me/code/app` istunnot ovat kansiossa
`sessions/--home-me-code-app--/`.

Projektin `.harness/` sisältää itse kirjoittamasi agentit, komennot, taidot ja
kehotepohjat, projektin asetukset sekä muistetut käyttöluvat. Projektin taidot
latautuvat myös hakemistosta `.agents/skills`, Harnessin taitojen jälkeen.

Aiemmat versiot tallensivat istunnot hakemistoon
`<projekti>/.agent-harness/sessions`. Niitä ei siirretä automaattisesti, mutta
ne aukeavat `--session-dir`-valitsimella:

```bash
harness sessions list --session-dir .agent-harness/sessions
```

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

Työtila koostuu seitsemästä Rust-cratesta. [Arkkitehtuuriohje](architecture/architecture.md)
kuvaa vastuut ja [käyttöliittymän suunnitteluohje](../DESIGN.md) päätteessä toimivat
komponentit.

```bash
cargo fmt --all -- --check
scripts/test-lanes.sh fast
scripts/test-lanes.sh quality-gates
```

Käytä Rust-testeihin nextestiä. [Testausohje](testing/testing.md) kuvaa muut
testiryhmät. Aloita asennusongelmissa [vianmäärityksestä](operations/troubleshooting.md).

## Lisenssi

Harness julkaistaan [MIT-lisenssillä](../LICENSE).
