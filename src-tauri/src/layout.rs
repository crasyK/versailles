use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn right(self) -> i32 {
        self.x + self.width
    }

    pub fn bottom(self) -> i32 {
        self.y + self.height
    }

    pub fn center_x(self) -> i32 {
        self.x + self.width / 2
    }

    pub fn center_y(self) -> i32 {
        self.y + self.height / 2
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum GuideOrientation {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SnapGuide {
    pub orientation: GuideOrientation,
    pub position: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SnapResult {
    pub x: i32,
    pub y: i32,
    pub guides: Vec<SnapGuide>,
}

/// Pure snapping: screen edges/centers + other widget edges/centers.
pub fn snap(
    candidate: Rect,
    others: &[Rect],
    monitors: &[Rect],
    threshold: i32,
) -> SnapResult {
    let mut x = candidate.x;
    let mut y = candidate.y;
    let mut guides = Vec::new();
    let mut best_dx: Option<(i32, i32, i32)> = None; // abs, new_x, guide
    let mut best_dy: Option<(i32, i32, i32)> = None;

    let mut x_pairs: Vec<(i32, i32, i32)> = Vec::new();
    let mut y_pairs: Vec<(i32, i32, i32)> = Vec::new();

    for monitor in monitors {
        x_pairs.push((candidate.x, monitor.x, monitor.x));
        x_pairs.push((candidate.right(), monitor.right(), monitor.right()));
        x_pairs.push((candidate.center_x(), monitor.center_x(), monitor.center_x()));
        y_pairs.push((candidate.y, monitor.y, monitor.y));
        y_pairs.push((candidate.bottom(), monitor.bottom(), monitor.bottom()));
        y_pairs.push((candidate.center_y(), monitor.center_y(), monitor.center_y()));
    }

    for other in others {
        x_pairs.push((candidate.x, other.x, other.x));
        x_pairs.push((candidate.x, other.right(), other.right()));
        x_pairs.push((candidate.right(), other.x, other.x));
        x_pairs.push((candidate.right(), other.right(), other.right()));
        x_pairs.push((candidate.center_x(), other.center_x(), other.center_x()));

        y_pairs.push((candidate.y, other.y, other.y));
        y_pairs.push((candidate.y, other.bottom(), other.bottom()));
        y_pairs.push((candidate.bottom(), other.y, other.y));
        y_pairs.push((candidate.bottom(), other.bottom(), other.bottom()));
        y_pairs.push((candidate.center_y(), other.center_y(), other.center_y()));
    }

    for (source, target, guide_pos) in x_pairs {
        let delta = target - source;
        let abs = delta.abs();
        if abs <= threshold {
            let replace = best_dx.map(|(a, _, _)| abs < a).unwrap_or(true);
            if replace {
                best_dx = Some((abs, x + delta, guide_pos));
            }
        }
    }

    for (source, target, guide_pos) in y_pairs {
        let delta = target - source;
        let abs = delta.abs();
        if abs <= threshold {
            let replace = best_dy.map(|(a, _, _)| abs < a).unwrap_or(true);
            if replace {
                best_dy = Some((abs, y + delta, guide_pos));
            }
        }
    }

    if let Some((_, nx, guide_pos)) = best_dx {
        x = nx;
        guides.push(SnapGuide {
            orientation: GuideOrientation::Vertical,
            position: guide_pos,
        });
    }
    if let Some((_, ny, guide_pos)) = best_dy {
        y = ny;
        guides.push(SnapGuide {
            orientation: GuideOrientation::Horizontal,
            position: guide_pos,
        });
    }

    SnapResult { x, y, guides }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snaps_to_monitor_edge() {
        let candidate = Rect {
            x: 8,
            y: 100,
            width: 100,
            height: 50,
        };
        let monitors = [Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }];
        let result = snap(candidate, &[], &monitors, 12);
        assert_eq!(result.x, 0);
        assert!(result
            .guides
            .iter()
            .any(|g| g.orientation == GuideOrientation::Vertical && g.position == 0));
    }

    #[test]
    fn snaps_to_other_widget() {
        let candidate = Rect {
            x: 210,
            y: 40,
            width: 100,
            height: 50,
        };
        let others = [Rect {
            x: 40,
            y: 40,
            width: 160,
            height: 50,
        }];
        let monitors = [Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }];
        let result = snap(candidate, &others, &monitors, 12);
        assert_eq!(result.x, 200);
    }

    #[test]
    fn no_snap_outside_threshold() {
        let candidate = Rect {
            x: 40,
            y: 40,
            width: 100,
            height: 50,
        };
        let monitors = [Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }];
        let result = snap(candidate, &[], &monitors, 12);
        assert_eq!(result.x, 40);
        assert_eq!(result.y, 40);
        assert!(result.guides.is_empty());
    }
}
