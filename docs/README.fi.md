<div align="center">
  <h1>agent-harness</h1>
  <p><strong>Tapahtumalähteinen, päätteessä toimiva harness agenteille, joiden täytyy toimia, delegoida ja jättää toistettava jälki.</strong></p>
  <p>
    <code>Rust</code> · <code>Ratatui</code> · <code>monipalveluntarjoaja</code> · <code>paikallinen ensin</code>
  </p>
  <p>
    <a href="#get-started">Aloita</a> ·
    <a href="#configure">Määritä</a> ·
    <a href="#operate">Käytä</a> ·
    <a href="configuration/config.md">Viite</a>
  </p>
</div>

<p align="center">
  <img src="assets/harness-tui.png" alt="Harnessin päätekäyttöliittymä, jossa näkyy kirjoituspainotteinen istuntoikkuna." width="960" />
</p>

<p align="center"><em>Paikallinen, offline-tilassa otettu TUI-esikatselu. Käynnistä vuorovaikutteinen käyttöliittymä ajamalla <code>harness</code> ilman alikomentoa.</em></p>

Harness on komentorivityökalu ja päätekäyttöliittymä koodausagenttien ajamiseen. Yksi koordinaattori vastaa aikataulutuksesta, oikeuksista, työkalujen suorituksesta, istuntohistoriasta ja palautumisesta. Se on tarkoitettu ihmisille, jotka haluavat kyvykkään mutta läpinäkyvän agentin: jokainen ajo tallennetaan vain lisättävistä tapahtumista koostuvana ketjuna, ja toisto lukee tapahtumat suorittamatta työkaluja, koukkuja, palveluntarjoajia tai verkkokutsuja uudelleen.

| Tarvitset | Harness tarjoaa |
| --- | --- |
| Luotettavan vuorovaikutteisen agentin | Kirjoituspainotteisen Ratatui-kuoren, mallin ja agentin vaihdon, oikeuspyynnöt sekä istuntonavigoinnin. |
| Skriptattavan automaation | `run`-komennon ilman käyttöliittymää, kohdennetun yhteensopivuuspinnan `prompt`-komennolla ja deterministiset mock-ajot offline-tarkistuksiin. |
| Hallitun delegoinnin | Ensiluokkaiset `task`-kutsut, tarkoitukseen tehdyt agenttiprofiilit, kategoriareitit ja koordinaattorin hallitsemat taustatehtävien ohjaimet. |
| Jäljitettävän historian | Redaktoidut tapahtumalokit, sivuvaikutuksista vapaan toiston ja tarkastelun, sukulinjan sekä tukiviennit. |
| Ymmärrettävät asetukset | JSONC-ajonaikaiset ja TUI-asetukset, kerroksittaisen haun, asetusten alkuperätiedot ja salaisuuksille turvallisen diagnostiikan. |

<a id="get-started"></a>

## Aloita

### 1. Rakenna lähdekoodista

Tarvitset projektiin kiinnitetyn Rust-työkaluketjun ja `git`in. Kloonaa työtila, rakenna komentorivityökalu ja luo sitten puhdas ensiajo-hakemisto mukana toimitetulla aloitusasetuksella:

```bash
git clone <repo-url> agent-harness
cd agent-harness
cargo build -p harness

export HARNESS_BIN="$PWD/target/debug/harness"
mkdir -p /tmp/harness-first-run
cp configs/harness.example.jsonc /tmp/harness-first-run/harness.jsonc
cd /tmp/harness-first-run
```

### 2. Tarkista ennen palveluntarjoajaan yhdistämistä

Aloitusasetus käyttää sisäänrakennettua `openai-codex`-palveluntarjoajaa. Varmista ensin, että paikallinen asennus on yhtenäinen:

```bash
"$HARNESS_BIN" --version
"$HARNESS_BIN" config validate
"$HARNESS_BIN" doctor
```

`doctor` tarkistaa paikallisen valmiuden: asetukset, palveluntarjoaja- ja mallimetatiedot, tunnistetietojen saatavuuden, työkalut, promptit, oikeudet, istuntotallennuksen ja määritetyn MCP-rekisteröinnin. Se ei koskaan tee palveluntarjoaja- tai MCP-verkkopyyntöä, joten virheetön tulos ei todista, että todennus toimii verkossa.

### 3. Kokeile koko polku offline-tilassa

Aja deterministinen mock-vuoro ennen kuin käytät yhtään tokenia:

```bash
"$HARNESS_BIN" run --mock "Hello from Harness" \
  --out prompt.events.jsonl --print-run-dir
```

Tämä tarkistaa ensimmäisen promptin suorituspolun ja kirjoittaa tapahtumalokin. Se on tarkoituksella erillään tunnistetietojen ja verkkoyhteyden tarkistuksista.

### 4. Yhdistä ja aloita oikea istunto

Pidä tunnistetiedot poissa `harness.jsonc`-tiedostosta. Aloitusasetus käyttää Codex OAuthia ja tukee varavaihtoehtona `OPENAI_API_KEY`-ympäristömuuttujaa. Kirjaudu sisään ja käynnistä sitten päätekäyttöliittymä:

```bash
"$HARNESS_BIN" auth login codex
"$HARNESS_BIN"
```

Jos tarvitset sen sijaan kertaluonteisen promptin ilman käyttöliittymää, käytä:

```bash
"$HARNESS_BIN" run "Summarize the current workspace"
```

Tarkista palveluntarjoajan todennus ja verkkoyhteys yhdellä oikealla `run`-ajolla tai vuorovaikutteisella vuorolla. Jos se epäonnistuu, aloita [`doctor`-komennosta](operations/troubleshooting.md) ja [palveluntarjoajatuen oppaasta](configuration/provider-support.md).

<a id="configure"></a>

## Määritä Harness

Harness erottaa ajonaikaiset asetukset TUI-asetuksista:

| Tiedosto | Omistaa | Aloita tästä |
| --- | --- | --- |
| `harness.jsonc` | Palveluntarjoajat, mallit, agentit, oikeudet, muotoilijat, taidot ja MCP-palvelimet | [`configs/harness.example.jsonc`](../configs/harness.example.jsonc) |
| `tui.jsonc` | Vain näppäinsidonnat | [`configs/tui.example.jsonc`](../configs/tui.example.jsonc) |

Kopioi aloitusasetus ja säädä sitten niitä harvoja päätöksiä, jotka todella muovaavat päivittäistä käyttöä:

```jsonc
{
  // Istuntojen oletuspalveluntarjoaja ja -malli.
  "model": "openai-codex/gpt-5.4-mini",

  // Agentit voivat käyttää suoraa mallia tai nimettyä malliprofiilia.
  "agent": {
    "build": { "variant": "high" },
    "explore": { "enable": false }
  },

  // Tee arkaluonteisesta työstä eksplisiittistä. Viimeisin täsmäävä bash-sääntö voittaa.
  "permission": {
    "edit": "ask",
    "bash": {
      "git *": "allow",
      "cargo test*": "ask",
      "*": "deny"
    },
    "webfetch": "deny"
  }
}
```

Katkelma näyttää kopioituun aloitusasetukseen tehtävät muutokset, ei itsenäistä asetustiedostoa. Aloitusasetus sisältää palveluntarjoajaluettelon ja agenttimäärittelyt, joihin nämä asetukset viittaavat.

### Mitä kannattaa määrittää ensin

| Asetus | Miksi se on tärkeä |
| --- | --- |
| `provider` ja `model` | Määrittävät käytettävissä olevan palveluntarjoaja- ja malliluettelon sekä aktiivisen oletuksen. |
| `model_profile` | Nimeää uudelleenkäytettävät malli- ja päättelyvarianttireitit kategoria-agenteille ja varavaihtoehdoille. |
| `agent` ja `default_agent` | Ottavat käyttöön, poistavat käytöstä ja säätävät Build-, Plan-, Explore-, General- ja kategoriaprofiileja. |
| `permission` | Päättää, sallitaanko sisäänrakennettujen työkalujen kyvykkyydet, kysytäänkö niistä vai kielletäänkö ne. |
| `formatter` | Hallitsee muokkausten jälkeisiä muotoilijoita; jätä pois, jos haluat pitää sisäänrakennetun muotoilijarekisterin käytössä. |
| `mcp` | Rekisteröi käytössä olevat, asetuksiin perustuvat MCP-palvelimet ajonaikaiseen työkalurekisteriin. |

[Täydellinen asetusten viite](configuration/config.md) dokumentoi jokaisen julkisen avaimen ja sen validointikäytöksen. Lue [oikeusopas](permissions/permissions.md), kun tarvitset täsmällisen oikeussanaston ja sääntöjoukon semantiikan.

### Selvitä, mistä asetus tulee

Ajonaikaiset asetuskerrokset yhdistyvät yhteisistä oletuksista projektikohtaisiin asetuksiin. Kanoniset sijainnit ovat XDG:n yleiset asetukset, projektin `harness.json{,c}` ja `.agent-harness/harness.json{,c}`, joita etsitään kohti projektijuurta. Eksplisiittiset ympäristöpeiteasetukset voivat olla viimeisenä etusijalla. Markdown-muotoiset agenttiprofiilit hakemistossa `.agent-harness/agents/` löytyvät tästä riippumatta.

Älä arvaa, mikä tiedosto voitti. Kysy komentorivityökalulta:

```bash
"$HARNESS_BIN" config show --effective
"$HARNESS_BIN" config sources
"$HARNESS_BIN" config explain model
"$HARNESS_BIN" config settings
```

Lopullinen näkymä redaktoi salaisuuksia sisältävät arvot. `sources` näyttää yhdistämisjärjestyksen, `explain` kohdistaa yhden piste-erotellun avaimen voittaneeseen kerrokseen ja `settings` listaa tyypitetyt metatiedot ilman salaisia arvoja.

### Määritä päätekäyttöliittymä erikseen

Tallenna näppäimistöasetukset tiedostoon `tui.jsonc`; ne eivät koskaan jaa ajonaikaisten asetusten pintaa:

```jsonc
{
  "keybinds": {
    "leader": "ctrl+x",
    "palette": "ctrl+p, <leader>p",
    "switch_model": "<leader>m",
    "open_lineage_browser": "<leader>g"
  }
}
```

Katso kaikki toiminto-ID:t ja oletussidonnat [TUI-asetusten viitteestä](configuration/config.md#tui-top-level-keys).

<a id="operate"></a>

## Käytä hallitusti

### Valitse sopiva käyttöliittymä

| Tavoite | Käytä tätä |
| --- | --- |
| Työskentele vuorovaikutteisesti | `harness` — Build on valittu oletuksena; siirry ensisijaisten agenttien välillä `Tab`-näppäimellä. |
| Ajattele ennen tiedostojen muuttamista | Vaihda Planiin. Se voi tarkastella työtilaa ja kirjoittaa aktiivisen suunnitelmansa, minkä jälkeen se pyytää siirtämään työn takaisin Buildille. |
| Aja CI:ssä tai skriptissä | `harness run "<prompt>"` |
| Kokeile kohdennettua alemman tason prompt-suorituspolkua | `harness prompt --text "<prompt>" --out events.jsonl` |
| Delegoi rajattu työ | Pyydä agenttia kutsumaan kanonista `task`-työkalua eksplisiittisellä promptilla, taustavalinnalla ja valinnaisella taitoluettelolla. |

[Agenttien ja ala-agenttien opas](operations/agents-and-subagents.md) selittää mukana toimitetut profiilit, kategoriavaravaihtoehdon, delegointipyynnön rakenteen ja ajonaikaiset rajat, jotka estävät työntekijää ohittamasta uudelleendelegoinnin rajoituksia.

### Pidä istunnot tarkasteltavina

Harness käsittelee tapahtumia totuuden lähteenä. Istuntotyökalut ja komentorivityökalu tarkastelevat vain toistosta johdettua dataa; ne eivät jatka palveluntarjoajaa, kutsu työkaluja, käynnistä MCP-palvelimia tai tee verkkokutsuja.

```bash
"$HARNESS_BIN" sessions list
"$HARNESS_BIN" sessions inspect <run-id-or-path>
"$HARNESS_BIN" sessions export \
  --session-dir <session-dir> \
  --output support-bundle.json \
  <run-id-or-directory-name>
```

Käytä tukivientiä raakojen tapahtumien jakamisen sijaan. Se sisältää toistosta johdetut metatiedot, redaktointimanifestin, ei-salaiset asetusten yhteenvedot ja salaisuusskannauksen tuloksen. Lue lisää [istunnoista ja toistosta](architecture/sessions-and-replay.md) sekä [tietosuojasta ja paikallisesta datasta](permissions/privacy-and-local-data.md).

### Tunne turvaraja

Koordinaattori ratkaisee oikeudet ennen kuin natiivityökalu suoritetaan. Kanoniset julkiset oikeusnimet ovat `bash`, `edit`, `question`, `task`, `webfetch`, `websearch`, `codesearch` ja `lsp`. Työkalun tuloste tallennetaan osana tapahtumahistoriaa, kun taas palveluntarjoajan metatiedot ja tukiartifaktit redaktoidaan.

Katso työkalukohtaiset kyvykkyydet ja toistokäytös [natiivityökalujen luettelosta](tools/native-tool-catalog.md). Tärkeät rajat — kuten miksi toisto ei koskaan suorita työkalua — kuvataan [arkkitehtuurissa](architecture/architecture.md).

## Kehitä ja varmista

Tietovarastossa on nopeat deterministiset tarkistukset päivittäiseen työhön sekä erilliset hyväksyntäkaistat PTY:tä, oikeaa palveluntarjoajaa ja natiivia visuaalista näyttöä koskeville todisteille:

```bash
scripts/test-lanes.sh fast
scripts/test-lanes.sh quality-gates
scripts/test-lanes.sh integration
scripts/test-lanes.sh all-deterministic
```

Kun muutos koskee ajonaikaista toimintaa, CLI:tä, työkaluja, skenaarioita tai istuntopolkua, suorita myös offline-mock-dogfood-polku:

```bash
bash scripts/harness-qa-dogfood.sh --self-test
```

[Testaus- ja hyväksyntäkartta](testing/testing.md) kertoo tarkasti, mitä kukin kaista todistaa — ja aivan yhtä tärkeää, mitä se ei todista.

## Vianmäärityksen pikapolut

| Oire | Ensimmäinen toimi |
| --- | --- |
| Asetusmuutos näyttää jäävän huomiotta | Aja `config sources` ja `config explain <path>`. |
| `doctor` onnistuu, mutta promptit epäonnistuvat | Aja yksi oikea prompt; `doctor` ei tarkoituksella testaa todennusta tai verkkoyhteyttä. |
| Työkalu kielletään | Tarkista ratkaistu `permission`-käytäntö ja työkalun julkinen oikeusluokka. |
| Istunto ei jatku | Tarkastele sitä vain-luku-tilassa komennolla `sessions inspect`, ja vie sitten redaktoitu tukipaketti. |
| Päätekäyttöliittymä näyttää väärältä | Yritä uudelleen `--mock`-valinnalla, avaa komentopaletti näppäimillä `Ctrl+p` ja tallenna pääteyksityiskohdat tukivientiin. |

[Ensiajon vianmääritysopas](operations/troubleshooting.md) sisältää pidemmät diagnoosipolut.

## Tutustu projektiin

| Alue | Aloita tästä |
| --- | --- |
| Asetukset ja palveluntarjoajat | [Asetusviite](configuration/config.md) · [Palveluntarjoajatuki](configuration/provider-support.md) |
| Työkalut ja oikeudet | [Natiivityökalujen luettelo](tools/native-tool-catalog.md) · [Oikeudet](permissions/permissions.md) |
| Istunnot ja palautuminen | [Istunnot ja toisto](architecture/sessions-and-replay.md) · [Tietosuoja](permissions/privacy-and-local-data.md) |
| Arkkitehtuuri | [Crate-rajojen ja invarianttien kuvaus](architecture/architecture.md) |
| Testaus | [Testaus ja hyväksyntä](testing/testing.md) |
