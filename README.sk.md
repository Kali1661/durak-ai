# Durak AI

[English](README.md) · **Slovensky**

Webová aplikácia pre kartovú hru [Durak](https://sk.wikipedia.org/wiki/Durak)
pre **dvoch alebo troch hráčov**. Môžeš hrať proti počítaču, alebo sledovať
hru, ktorú hráš inde, a dostávať odporúčania ťahov.

Všetko beží v tvojom prehliadači: herný engine je napísaný v Ruste a
skompilovaný do WebAssembly a na žiadny server sa nič neposiela.

Rozhranie aplikácie je v angličtine.

## Čo aplikácia dokáže

**Hra proti počítaču (Play the computer).** Aplikácia rozdá karty a ty hráš
proti jednému alebo dvom počítačovým súperom. Každý súper buď hľadá dobrý ťah
(nastavíš, ako dlho smie premýšľať), alebo hrá náhodne. Odporúčania pre tvoje
vlastné ťahy sa dajú kedykoľvek zapnúť alebo vypnúť.

**Analýza skutočnej hry (Analyse a real game).** Zadáš tromfovú kartu, svojich
šesť kariet a kto útočí ako prvý, a potom zaznamenávaš každý ťah tak, ako sa pri
stole odohrá. Vždy, keď si na ťahu, aplikácia zoradí tvoje možné ťahy. Keď si
doberieš karty, povieš aplikácii ktoré; keď je balíček prázdny a neznáme sú už
len karty jedného súpera, aplikácia ich dopočíta sama.

V oboch režimoch môžeš vracať ťahy, hry sa ukladajú v prehliadači a po návrate
sa obnovia. Aplikácia funguje na telefóne, tablete aj počítači, vo svetlom aj
tmavom vzhľade.

## Pravidlá

- 2 alebo 3 hráči; balíčky s 24 (od deviatky po eso), 36 (od šestky po eso) alebo 52 kartami; v ruke šesť
  kariet.
- Útočí sa po jednej karte; obranca odpovie skôr, než príde ďalšia.
- Najviac šesť útočných kariet v jednom kole a nikdy viac nepobitých kariet,
  než má obranca v ruke.
- Prihadzovanie: každý útočník môže pridať karty, ktorých hodnota už je na
  stole.
- Po kole si najprv doberajú útočníci (hlavný útočník prvý), obranca posledný.
  Obranca, ktorý karty zobral, prichádza o svoj útok.
- V hre rozdanej počítačom útočí ako prvý ten, kto má najnižší tromf.
- Hra končí, keď je balíček prázdny a karty drží najviac jeden hráč. Ten je
  durak. Ak posledný obranca pobije posledný útok svojou poslednou kartou,
  nikto nedrží karty a hra končí **remízou**.

Dve voliteľné pravidlá, ktoré sa vyberajú pri začatí hry:

- **Prekladanie (Reflecting)** - kým obranca ešte nič nepobil, môže zahrať
  kartu rovnakej hodnoty a presunúť celý útok na ďalšieho hráča, ak má ten
  dosť kariet, aby naň odpovedal. Pri dvoch hráčoch sa útok vracia pôvodnému
  útočníkovi.
- **Prekladanie ukázaním tromfu (Reflecting by showing a trump)** - obranca
  môže namiesto toho ukázať tromf danej hodnoty a ponechať si ho. Každý tromf
  sa dá ukázať raz za kolo.

## O odporúčaniach

Odporúčania pochádzajú z vyhľadávania Monte Carlo (MCTS). Karty, ktoré nevidíš,
sa mnohokrát náhodne rozdajú; každé rozdanie sa prehľadá a dohrá do konca.
Percento pri ťahu je jeho **skóre**: ako často si v týchto simulovaných hrách
neskončil ako durak (remíza sa počíta ako neprehra; simulácia prerušená pred
koncom sa počíta ako polovica).

Skóre ber ako spôsob porovnania ťahov, nie ako presnú pravdepodobnosť. Závisí
od náhodných rozdaní a z veľkej časti náhodného dohrávania, medzi behmi sa
mení a vyhľadávanie sa môže mýliť. Pri každom skóre je uvedený počet simulácií,
aby si vedel posúdiť, nakoľko je ustálené; tlačidlo „Search longer" ich pridá.

## Spustenie

**Začínaš? Postupuj podľa [podrobného návodu na inštaláciu](INSTALL.sk.md).**
Pokrýva inštaláciu nástrojov, stiahnutie projektu aj riešenie bežných chýb.

V skratke, ak máš nainštalovaný [Git](https://git-scm.com/),
[Node.js](https://nodejs.org/) 18+, [Rust](https://rustup.rs/) a
[wasm-pack](https://rustwasm.github.io/wasm-pack/installer/):

```bash
git clone https://github.com/Kali1661/durak-ai.git
cd durak-ai/durak-online/web
npm install
npm run dev
```

a otvor <http://localhost:5173>. Každý riadok spusti samostatne a pred ďalším
over, že prebehol. Po inštalácii nástroja otvor nové okno terminálu.

## Štruktúra projektu

```
durak-online/
├── durak-core/   engine v Ruste: pravidlá, validácia, vyhľadávanie  (testy v tests/)
├── durak-wasm/   väzby pre WebAssembly
└── web/          aplikácia vo Svelte                                (testy v tests/)
```

## Vzťah k pôvodnému projektu

Tento projekt vznikol z
[durak-online](https://github.com/jorisperrenet/durak-online), ktorého autorom
je Joris Perrenet, a zachováva jeho celkový návrh: model hry, vyhľadávanie
Monte Carlo s náhodným rozdaním skrytých kariet bežiace vo WebAssembly a
obrázky kariet.

Väčšina kódu bola odvtedy prepísaná a vylepšená. Engine bol prepracovaný tak,
aby overoval každý vstup aj uloženú hru, kontroloval svoje pravidlá
automatickými testami a hodnotil pozície pre každého hráča zvlášť.
Vyhľadávanie, webová aplikácia aj rozhranie boli vytvorené nanovo.

Oproti pôvodnej verzii táto verzia:

- podporuje iba stoly pre dvoch a troch hráčov;
- v skutočnej hre umožňuje zvoliť, kto útočí ako prvý;
- rozpoznáva remízy a dodržiava limit šiestich kariet na kolo;
- pri každom odporúčaní ukazuje, koľko simulácií za ním stojí;
- má nové rozhranie s rozložením pre telefón a podporou klávesnice a čítačiek
  obrazovky.

Prípadné chyby v tejto verzii idú na jej vrub, nie na vrub pôvodného autora.

## Poďakovanie a licencia

Herný engine a pôvodnú webovú aplikáciu vytvoril
[Joris Perrenet](https://github.com/jorisperrenet/durak-online) a sú použité
pod licenciou MIT; pozri [`durak-online/LICENSE`](durak-online/LICENSE). Zmeny
v tomto repozitári sú zverejnené pod rovnakou licenciou.

Obrázky kariet pochádzajú z pôvodného projektu.
