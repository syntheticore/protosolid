use super::*;

/// The normal offset of a full circular G2 band is the revolution of its
/// offset meridian. Trim any loops in that meridian before publishing faces.
pub(super) struct CircularBlendOffset {
    frame: crate::Frame3,
    profile: Curve2,
    bands: Vec<ParameterRange>,
}

impl CircularBlendOffset {
    pub(super) fn new(
        topology: &Topology,
        face: FaceId,
        openings: &BTreeSet<FaceId>,
        blend: &crate::EdgeBlendSurface3,
        bounds: [ParameterRange; 2],
        distance: f64,
        tolerance: crate::ModelTolerance,
    ) -> KernelResult<Option<Self>> {
        let period = std::f64::consts::TAU;
        if (bounds[0].span() - period).abs() > tolerance.parameter * 64.0
            || bounds[1].start.abs() > tolerance.parameter * 64.0
            || (bounds[1].end - 1.0).abs() > tolerance.parameter * 64.0
        {
            return Ok(None);
        }
        let edges = topology.face_edges(face)?;
        if edges.len() != 3 {
            return Ok(None);
        }
        for edge in edges {
            if topology
                .edge_faces(edge)?
                .iter()
                .any(|id| openings.contains(id))
            {
                // Both contact rails must follow the same normal offset.
                return Ok(None);
            }
        }
        let Some((controls, axis)) = blend.rotating_quintic(0.0)? else {
            return Ok(None);
        };
        let Curve3::Circle(spine) = blend.center_curve() else {
            unreachable!()
        };
        let frame =
            crate::Frame3::new(axis.origin(), spine.x_radius(), spine.y_radius(), tolerance)?;
        let controls = controls.map(|point| {
            let delta = point - frame.origin();
            Point2::new(delta.dot(frame.x()), delta.dot(frame.z()))
        });
        let meridian: Curve2 = crate::NurbsCurve2::new(
            5,
            controls.to_vec(),
            vec![1.0; 6],
            [vec![0.0; 6], vec![1.0; 6]].concat(),
        )?
        .into();
        // In the (radius, height) chart the surface normal points to the
        // right of the meridian, while Curve2 offsets are positive to the left.
        let profile: Curve2 = crate::OffsetCurve2::new(meridian.clone(), -distance)?.into();
        let full = ParameterRange::new(0.0, 1.0)?;
        let speed_ratio = |t| -> KernelResult<f64> {
            let tangent = meridian.first_order(t)?.1;
            Ok(profile.first_order(t)?.1.dot(tangent) / tangent.magnitude2())
        };
        // The right-angle section is symmetric, with one curvature maximum
        // in each half. Locate the first to detect even a narrow fold.
        let (mut low, mut high) = (0.0, 0.5);
        for _ in 0..32 {
            let a = (2.0 * low + high) / 3.0;
            let b = (low + 2.0 * high) / 3.0;
            if speed_ratio(a)? < speed_ratio(b)? {
                high = b;
            } else {
                low = a;
            }
        }
        let bands = if speed_ratio((low + high) * 0.5)? > 0.0 {
            vec![full]
        } else {
            let offset = TrimmedCurve2::new(profile.clone(), full)?;
            let mut loops = Vec::new();
            for hit in offset.self_intersections(tolerance)? {
                if let crate::CurveIntersection2::Point {
                    first_parameter,
                    second_parameter,
                    ..
                } = hit
                {
                    let start = first_parameter.min(second_parameter);
                    let end = first_parameter.max(second_parameter);
                    if speed_ratio(start)? > 0.0 && speed_ratio(end)? > 0.0 {
                        loops.push((start, end));
                    }
                }
            }
            loops.sort_by(|a, b| a.0.total_cmp(&b.0).then(b.1.total_cmp(&a.1)));
            let mut bands = Vec::new();
            let mut start = 0.0;
            for (first, second) in loops {
                if first > start + tolerance.parameter {
                    bands.push(ParameterRange::new(start, first)?);
                    start = second;
                }
            }
            bands.push(ParameterRange::new(start, 1.0)?);
            bands
        };
        Ok(Some(Self {
            frame,
            profile,
            bands,
        }))
    }

    pub(super) fn is_split(&self) -> bool {
        self.bands.len() > 1
    }

    fn surface(
        &self,
        band: ParameterRange,
        tolerance: crate::ModelTolerance,
    ) -> KernelResult<Surface3> {
        crate::RevolvedSurface3::new(self.frame, self.profile.clone(), band, tolerance)
            .map(Into::into)
    }

    pub(super) fn unsplit_surface(
        &self,
        tolerance: crate::ModelTolerance,
    ) -> KernelResult<Surface3> {
        self.surface(self.bands[0], tolerance)
    }

    pub(super) fn patches(
        &self,
        patch: &BrepPatch,
        tolerance: crate::ModelTolerance,
    ) -> KernelResult<Vec<BrepPatch>> {
        let boundary = &patch.loops[0].segments;
        let mut rails = [None; 2];
        for edge in boundary {
            let start = edge.pcurve.first_order(edge.range.start)?.0;
            let end = edge.pcurve.first_order(edge.range.end)?.0;
            if (start.x - end.x).abs() > std::f64::consts::PI {
                rails[usize::from(start.y > 0.5)] = Some((edge, start.x, end.x));
            }
        }
        let [Some((bottom, start, end)), Some((top, _, _))] = rails else {
            return Err(topology_error(
                "the offset blend must retain both circular contact rails",
            ));
        };
        let angular = ParameterRange::new(start, end)?;
        self.bands
            .iter()
            .map(|&band| {
                let surface = self.surface(band, tolerance)?;
                let segment = |origin, direction, range| -> KernelResult<BrepPatchSegment> {
                    let pcurve: Curve2 = crate::Line2::new(origin, direction)?.into();
                    let curve = surface.boundary_curve(pcurve.clone(), range, tolerance)?;
                    Ok(BrepPatchSegment {
                        curve,
                        pcurve,
                        range,
                        provenance: Default::default(),
                    })
                };
                let lower = if band.start == 0.0 {
                    bottom.clone()
                } else {
                    segment(Point2::new(0.0, band.start), crate::Vec2::unit_x(), angular)?
                };
                let upper = if band.end == 1.0 {
                    top.clone()
                } else {
                    segment(
                        Point2::new(0.0, band.end),
                        crate::Vec2::unit_x(),
                        angular.reversed(),
                    )?
                };
                Ok(BrepPatch {
                    sources: patch.sources.clone(),
                    preserved: Vec::new(),
                    orientation: patch.orientation,
                    loops: vec![BrepPatchLoop {
                        kind: crate::LoopKind::Outer,
                        segments: vec![
                            lower,
                            segment(Point2::new(end, 0.0), crate::Vec2::unit_y(), band)?,
                            upper,
                            segment(
                                Point2::new(start, 0.0),
                                crate::Vec2::unit_y(),
                                band.reversed(),
                            )?,
                        ],
                    }],
                    surface,
                })
            })
            .collect()
    }
}
