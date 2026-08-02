# Vaihtoehdot ja valinnat

| Valinta | Perustelu ja jatko |
| --- | --- |
| Vain loppuun kasvava tapahtumaloki ja projektiot | Suoraan muuttuva ajonaikainen tila olisi ollut aluksi yksinkertaisempi. Päädyin tapahtumalokiin, koska sama ajo voidaan rakentaa siitä uudelleen ilman uusia sivuvaikutuksia. Seuraavaksi haluan mitata replayn kustannuksen erittäin pitkillä sessioilla. |
| Yksi koordinaattori | Ajoituksen olisi voinut hajauttaa useaan kerrokseen. Yksi koordinaattori pitää käyttöoikeudet, tapahtumat ja elinkaaren samassa järjestyksessä, joten virheitä on helpompi jäljittää. |
| Hash-ankkuroitu atominen muokkaus | Pelkkiin rivinumeroihin perustuva muokkaus olisi ollut helpompi toteuttaa. Sisältöön sidottu ankkuri kestää paremmin rinnakkaisia muutoksia ja mahdollistaa ristiriitojen hylkäämisen ennen kirjoitusta. |
| Deterministinen testi ja replay | Pelkät live-testit olisivat riippuvaisia verkosta, kustannuksista ja salaisuuksista. Testikaksoisolennot ja tallennetut virrat tekevät testistä toistettavan, mutta eivät poista erillisen live-testauksen tarvetta. |
| Ratatui TUI | Erillinen graafinen replay-näkymä olisi vienyt huomion pois ajonaikaisesta ytimestä. TUI antoi rajatussa ajassa toimivan, näppäimistöllä käytettävän pinnan. Viimeistelen sen ennen uuden käyttöliittymän aloittamista. |
| Rust-työtila crateineen | Yksi suuri crate olisi vähentänyt alkuvaiheen rakennetta. Jaettu työtila teki vastuut näkyviksi kääntäjälle ja helpotti muutosten rajaamista. |

Mikään näistä valinnoista ei ollut itsestään selvä. Hyödyllisintä oli huomata, että hylätty vaihtoehto kannattaa kirjoittaa näkyviin. Muuten sama päätös joudutaan perustelemaan aina uudelleen.
