# Oppimispäiväkirjan designjärjestelmä

## 0. Tutkimusloki

- Konkreettinen sopimus: KAMKin oppimispäiväkirjapohjan MkDocs Material- ja awesome-nav-rakenne on säilytetty. Se ratkaisee ulkoasun ilman omaa brändijäljitelmää.
- Layer B: erillistä brändiviitettä ei käytetä, koska annettu pohja on tämän sivuston ainoa visuaalinen sopimus.
- Ulkoinen ruutukuvatutkimus ja kuvakonseptit jätettiin tekemättä. Sivusto on dokumentaatio, ja niiden tulos rikkoisi pyydetyn pohjan rauhallista sisältöedellä rakennettua luonnetta.

## 1. Tunnelma ja identiteetti

Sivusto on rauhallinen, luettava ja näyttöön nojaava oppimispäiväkirja. Tunnistettava ratkaisu on selkeä dokumentaatiohierarkia, jossa etusivu kertoo kokonaisuuden ja viikkonavigaatio pitää 23 merkintää löydettävinä. En jäljittele omaa tai ulkopuolista brändiä.

## 2. Värit

Värit tulevat MkDocs Materialin valmiista blue grey -paletista. Valitsin paletin asetuksena, en omilla väriarvoilla tai mukautetulla CSS:llä, jotta leipätekstin linkit erottuvat paremmin tekstistä. Linkit, huomautuslaatikot ja fokustilat käyttävät teeman valmiita, semanttisia tiloja. Generoijan alatunniste on poistettu teeman omalla asetuksella, koska sen kontrasti ei täyttänyt tarkistuksen vaatimusta.

## 3. Typografia

Typografia tulee Material-teeman oletuksista. Otsikot, leipäteksti, taulukot ja koodilohkot säilyttävät teeman responsiivisen mittakaavan. Pitkät tekniset kohdat ja taulukot jaetaan otsikoilla ja tiiviillä kappaleilla, jotta lukeminen pysyy hallittavana myös kapealla näytöllä.

## 4. Tila ja asettelu

Materiaalin dokumentaatioasettelu on sivuston ruudukko. Sisältö pysyy yhden pääpalstan lukuleveydessä, navigaatio jää teeman hallintaan ja taulukot ovat Markdown-taulukoita. Leveissä taulukoissa tieto on tiivistetty niin, että vaakasuuntainen vieritys on viimeinen eikä ensisijainen keino.

## 5. Käyttöliittymäosat

### Pää- ja viikkonavigaatio

- **Rakenne:** Materialin otsake, sivupalkki ja awesome-navin järjestämä sivupuu.
- **Tilat:** teeman oletus-, osoitin-, aktiivinen- ja näppäimistöfokustila.
- **Saavutettavuus:** näppäimistönavigointi, ohitussisältölinkki ja kohdistusilmaisin periytyvät Materialilta.

### Tietotaulukko

- **Rakenne:** Markdown-taulukko otsikkorivillä.
- **Tilat:** staattinen sisältö.
- **Saavutettavuus:** otsikot ilmaisevat sarakkeiden merkityksen ja tekstisisältö ei nojaa väriin.

### Todennettavat linkit

- **Rakenne:** sivun jatkolinkit ovat lihavoituja ja commit-tunnukset ovat linkitettyjä koodispaneja.
- **Saavutettavuus:** rakenne erottaa linkin ympäröivästä tekstistä ilman pelkkää väriä.

### Huomautuslaatikko

- **Rakenne:** Materialin native admonition-syntaksi.
- **Tilat:** informaatio ja varoitus.
- **Saavutettavuus:** otsikko kertoo laatikon tarkoituksen myös ilman väriä.

### Mermaid-kaavio

- **Rakenne:** Markdownin aidattu Mermaid-lohko.
- **Tilat:** staattinen kaavio.
- **Saavutettavuus:** sama väite ja järjestys esitetään kaavion vieressä tekstinä tai taulukkona.

## 6. Liike ja vuorovaikutus

En lisää koristeellista liikettä tai omaa JavaScriptiä. Materialin valmis, käyttäjän selaimessa toimiva navigointikäyttäytyminen jää käyttöön. Sivusto ei edellytä liikettä sisällön ymmärtämiseksi.

## 7. Syvyys ja pinnat

Pintojen, reunojen ja korostusten strategia on Material-teeman oletus. En lisää varjoja, kortteja tai omia pinnan erotteluja. Näin sisältö, otsikot ja todennettavat jäljet pysyvät ensisijaisina.

## 8. Saavutettavuusrajoitteet ja hyväksytty velka

Tavoite on WCAG 2.2 AA -tasoinen luettavuus Materialin natiiveilla kontrasti-, fokus- ja näppäimistöominaisuuksilla. Sivuston kieli on suomi, otsikkotasoja käytetään järjestyksessä ja kaavioiden tieto löytyy myös tekstistä. Mukautettua CSS:ää ei ole, joten värikontrastin ja fokuksen käyttäytyminen säilyy teeman ylläpidettävänä.

| Velka | Sijainti | Peruste | Omistaja ja poistumistapa |
| --- | --- | --- | --- |
| Ei hyväksyttyä design- tai saavutettavuusvelkaa | Koko sivusto | Tarkistetaan jokaisessa sisältömuutoksessa | Opiskelija, korjaa ennen seuraavaa julkaisua jos havainto syntyy |
