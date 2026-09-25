use crate::site::MAX_FLOORS;
use crate::Params;
use serde::{Deserialize, Serialize};

/// Plan-grid coordinate: (i east, j south).
pub type Cell = (u16, u16);

pub const NO_PLOT: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ground {
    Outside,
    Alley,
    /// The old magistrate's office. Never built over.
    Yamen,
    /// Light well / void left between buildings.
    Well,
    Plot,
}

/// What a column of a plot is used for (the same on every floor).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    None,
    Room,
    /// Stair landing: entered from the lane at ground level; doors and corridors
    /// attach here on every floor.
    Core,
    /// Stair shaft beside the landing: a steep dog-leg from each floor to the next,
    /// ending in a stair hut on the roof.
    Stair,
    /// Landing/corridor linking rooms to the core.
    Corridor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Dir {
    N,
    E,
    S,
    W,
}

impl Dir {
    pub const ALL: [Dir; 4] = [Dir::N, Dir::E, Dir::S, Dir::W];
    pub fn delta(self) -> (i32, i32) {
        match self {
            Dir::N => (0, -1),
            Dir::E => (1, 0),
            Dir::S => (0, 1),
            Dir::W => (-1, 0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FeatureKind {
    /// One of the eight municipal water pipes that supplied the whole city.
    WaterStandpipe,
    /// The last natural ground well, off Tai Chang ("Big Well") Street.
    NaturalWell,
    /// One of only two lifts in the city.
    Lift,
    Temple,
    SouthGate,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Feature {
    pub kind: FeatureKind,
    pub cell: Cell,
    pub name: Option<String>,
    pub plot: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plot {
    pub id: u32,
    pub cells: Vec<Cell>,
    /// `core[0]` is the landing (opens onto a lane at ground level), `core[1]` the stair.
    pub core: Vec<Cell>,
    /// Year the plot was first built on (0 = never).
    pub founded: u16,
    /// Year of the last wholesale rebuild (0 = original structure).
    pub rebuilt: u16,
    /// Build year of each floor (0 = not built). Floors are contiguous from 0.
    pub floor_year: [u16; MAX_FLOORS],
    /// Height this building aspires to.
    pub ambition: u8,
}

impl Plot {
    pub fn height_at(&self, year: u16) -> u8 {
        self.floor_year.iter().take_while(|&&y| y != 0 && y <= year).count() as u8
    }
    pub fn final_height(&self) -> u8 {
        self.floor_year.iter().take_while(|&&y| y != 0).count() as u8
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnitUse {
    Flat,
    Shop,
    Workshop,
    FishballFactory,
    Dentist,
    Clinic,
    Restaurant,
    Temple,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DoorState {
    Open,
    Closed,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Door {
    /// Cell inside the unit the door is in.
    pub cell: Cell,
    /// Wall the door is in; the neighbour that way is the core/corridor/alley it opens onto.
    pub facing: Dir,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Unit {
    pub id: u32,
    pub plot: u32,
    pub floor: u8,
    pub cells: Vec<Cell>,
    pub door: Door,
    pub usage: UnitUse,
    pub door_state: DoorState,
    /// Proper name, for the few places that had one (temples).
    pub name: Option<String>,
}

/// A walkway joining two buildings' circulation at the same floor. `span` holds the
/// alley cells it crosses (empty = walls knocked through between neighbours).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bridge {
    pub floor: u8,
    pub a: Cell,
    pub b: Cell,
    pub span: Vec<Cell>,
    pub year: u16,
}

/// A named trunk lane. Names are real Walled City lanes; placement is invented.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Lane {
    pub name: String,
    pub cells: Vec<Cell>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct City {
    pub params: Params,
    pub w: usize,
    pub d: usize,
    pub ground: Vec<Ground>,
    pub plot_of: Vec<u32>,
    pub role: Vec<Role>,
    pub plots: Vec<Plot>,
    pub units: Vec<Unit>,
    pub bridges: Vec<Bridge>,
    pub lanes: Vec<Lane>,
    pub features: Vec<Feature>,
    pub south_gate: Cell,
    /// Footprint in plan metres (x east, y south), for drawing the map.
    pub ring_m: Vec<(f32, f32)>,
    pub yamen_m: Vec<(f32, f32)>,
}

impl City {
    #[inline]
    pub fn idx(&self, c: Cell) -> usize {
        c.1 as usize * self.w + c.0 as usize
    }
    pub fn in_bounds(&self, i: i32, j: i32) -> bool {
        i >= 0 && j >= 0 && (i as usize) < self.w && (j as usize) < self.d
    }
    pub fn step(&self, c: Cell, d: Dir) -> Option<Cell> {
        let (di, dj) = d.delta();
        let (i, j) = (c.0 as i32 + di, c.1 as i32 + dj);
        self.in_bounds(i, j).then_some((i as u16, j as u16))
    }
    pub fn neighbours(&self, c: Cell) -> impl Iterator<Item = (Dir, Cell)> + '_ {
        Dir::ALL.into_iter().filter_map(move |d| self.step(c, d).map(|n| (d, n)))
    }
    pub fn ground_at(&self, c: Cell) -> Ground {
        self.ground[self.idx(c)]
    }
    pub fn role_at(&self, c: Cell) -> Role {
        self.role[self.idx(c)]
    }
    pub fn plot_at(&self, c: Cell) -> Option<&Plot> {
        let p = self.plot_of[self.idx(c)];
        (p != NO_PLOT).then(|| &self.plots[p as usize])
    }
    /// Storeys standing on this column in `year` (Yamen counts as 1).
    pub fn height_at(&self, c: Cell, year: u16) -> u8 {
        match self.ground_at(c) {
            Ground::Plot => self.plot_at(c).map_or(0, |p| p.height_at(year)),
            Ground::Yamen => 1,
            _ => 0,
        }
    }
    pub fn unit_year(&self, u: &Unit) -> u16 {
        self.plots[u.plot as usize].floor_year[u.floor as usize]
    }
    pub fn units_at(&self, year: u16) -> impl Iterator<Item = &Unit> + '_ {
        self.units.iter().filter(move |u| {
            let y = self.unit_year(u);
            y != 0 && y <= year
        })
    }
}
