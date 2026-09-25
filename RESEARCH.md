# Research notes: what's grounded and what's invented

## Sources
- Wikipedia, "Kowloon Walled City": https://en.wikipedia.org/wiki/Kowloon_Walled_City
- OpenStreetMap (© OpenStreetMap contributors, ODbL): Kowloon Walled City Park (relation 1650915), Former Yamen Building (relation 18506822), Remnants of the South Gate (way 263310044)
- Industrial History of Hong Kong Group, "Kowloon Walled City 2 – manufacturing": https://industrialhistoryhk.org/kowloon-walled-city-2/
- Hak Nam (Jefferson Duan, 2021): https://www.hak-nam.city/
- 1987 survey figures as quoted by Wikipedia and secondary sources (e.g. boingboing.net, 2026-07-07)
- Background (not yet used): Girard & Lambot, *City of Darkness* (1993) and *City of Darkness Revisited* (2014). These are the definitive sources for plans, sections and street-level detail.

## Figures the model is tested against (`tests/invariants.rs`)
| Fact | Value | Source |
|---|---|---|
| Site | 2.6 ha, about 210 × 120 m | Wikipedia |
| Buildings | about 350, "almost all between 10 and 14 storeys" | 1987 survey |
| Premises | 8,500 | 1987 survey |
| Households | 10,700 | 1987 survey |
| Residents | about 33,000 (1987); 35,000 (1990) | Wikipedia |
| Typical flat | 23 m² | Wikipedia |
| Height limit | 13–14 storeys (Kai Tak flight path, airport about 800 m south) | Wikipedia |
| Lanes | "often only 1–2 m" wide | Wikipedia |
| Water | 8 municipal pipes/standpipes for the whole city; one natural well off Tai Chang ("Big Well") Street | Wikipedia, IHHK |
| Lifts | 2 in the whole city; stairs linked many buildings; rooftops used as through-routes | Hak Nam |
| Entrances | "no real entrances … just narrow openings between shops" | Hak Nam |

## Placed from real geometry
- **Footprint:** the park outline shrunk 9.7 m all round to enclose 26,000 m². The park (3.1–3.3 ha) is the old city plus a buffer. The shrunk outline puts the South Gate ruins just inside the edge.
- **Yamen:** the convex hull of its three halls from OSM, plus 1.5 m, with a lane kept around it.
- **South Gate:** the centre of the excavated ruins.

## Named lanes (names real, courses invented unless noted)
| Lane | Course |
|---|---|
| Lung Chun Road | From the South Gate to the Yamen. This is the old axis, an assumption. |
| Lung Chun Back Road | West to east across the city ("one of the few alleyways that runs east–west across the city"). |
| Lo Yan Street | In from Tung Tau Tsuen Road on the north side. The real entrance "descended four storeys below ground level"; terrain isn't modelled yet. |
| Sai Shing ("West City") Road | North to south on the west side, an assumption from its name. |
| Tai Chang, Kwong Ming, Shing Ngam, Mung Chun and Lung Shing | Random openings. Lung Shing Road may really have been a boundary road. |

## Known gaps / next research
- **Real street plan.** City of Darkness has a ground-floor plan, and IHHK mentions an undated street map. Tracing the lane courses from those would replace the invented ones.
- **Terrain.** The site sloped, with the north side higher (the four-storey descent from Tung Tau Tsuen Road).
- **Lanes covered overhead.** Upper floors built over the lanes, and light rarely reached the ground. The renderer should close lanes over at upper floors.
- **Trades by street.** Lo Yan Street roast meats; fishball makers off Kwong Ming Street ("Electric Station", with stalls selling cheap drugs). Unit uses are currently random by floor and frontage.
- **Population history.** Census counts were 10,004 (1971) and 14,617 (1981), widely thought to undercount; the 1987 estimate was 33,000. The growth curve is invented to hit the 1987 end state.

## City of Darkness (Girard & Lambot, 1993): vibe reference for now
Used as a visual and atmosphere reference only; the generator isn't fitted to it yet. For a future accurate-map pass:
- **"The Map" (book pp. 214–215)** is a full ground plan: every building footprint, every named lane, the Yamen compound, and numbered places (standpipe 30, well 34, Tin Hau Temple 31, Fuk Tak Temple 12, Old People's Centre 26, Old School 27, St Stephen's 28). `tools/trace_map.py` already splits it into lanes and building pieces; it still needs merging and georeferencing (rotated about 15° from north; the Yamen compound is about 30 × 53 m on the map against 26 × 50 m in OSM).
- **Japanese survey cross-section** (east to west): Lung Shing Road is the east-side main passage, just inside the east wall, "one of the few lanes that gets daylight". Then dead-end passages, Kwong Ming Street (main), the Lung Chun 1st and 2nd Lanes, and Lo Yan Street (main, western half). There are open-air rubbish collection points at ground level.
- **Look and feel:**
  - the edge is a sheer 13–14-storey wall, with lower, ragged roofs inside;
  - the Yamen sits in an open pit;
  - roofs are a forest of TV aerials, with gardens, shacks and water tanks, and wiring runs down the walls;
  - facades are all cages, balconies and laundry;
  - shops have concertina shutters;
  - corridors and interiors are lit by fluorescent tubes (green-white), with pink walls inside.
- **Trades seen:** roast pork, noodles, bakery and steamed buns, toy plastics, textiles, toilet paper, sheet metal, herbalists, dentists (dentures in the window), mahjong on the 2nd–3rd floors, canteens, a triad office, an estate agent, and an elderly centre.
