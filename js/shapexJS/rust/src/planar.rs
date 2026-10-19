//! Private transport. Public naming, values and ownership live in ShapexJS.
use super::*;
use js_sys::{Array, Object, Reflect};
use shapex::kernel::{
    CurveContact, CurveIntersection2, JoinStyle, OffsetOptions, Path2, PlaneCurveProjection,
    PointClassification,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::{Rc, Weak},
};

thread_local! {
    static CURVES: RefCell<HashMap<u32,Weak<Curve2>>> = RefCell::new(HashMap::new());
    static NEXT_CURVE: Cell<u32> = const { Cell::new(0) };
}
pub(super) fn retained_curve(key: u32) -> Result<Curve2, JsValue> {
    CURVES
        .with(|curves| {
            curves
                .borrow()
                .get(&key)
                .and_then(Weak::upgrade)
                .map(|v| (*v).clone())
        })
        .ok_or_else(|| {
            JsValue::from_str(
                "retained curve belongs to a released value or another ShapexJS instance",
            )
        })
}
fn json(value: serde_json::Value) -> JsValue {
    JSON::parse(&value.to_string()).unwrap()
}
fn classify(value: PointClassification) -> String {
    match value {
        PointClassification::Inside => "inside",
        PointClassification::Outside => "outside",
        PointClassification::Boundary => "boundary",
    }
    .into()
}
fn intersection_values(hits: Vec<CurveIntersection2>) -> JsValue {
    json(serde_json::Value::Array(hits.into_iter().map(|hit|match hit {
        CurveIntersection2::Point { point,first_parameter,second_parameter,contact }=>serde_json::json!({"kind":"point","point":[point.x,point.y],"firstParameter":first_parameter,"secondParameter":second_parameter,"contact":if contact==CurveContact::Tangent {"tangent"} else {"crossing"}}),
        CurveIntersection2::Overlap { first,second }=>serde_json::json!({"kind":"overlap","first":[first.start,first.end],"second":[second.start,second.end]}),
    }).collect()))
}
fn join_style(join: &str) -> Result<JoinStyle, JsValue> {
    Ok(match join {
        "round" => JoinStyle::Round,
        "miter" => JoinStyle::Miter,
        "bevel" => JoinStyle::Bevel,
        _ => return Err(JsValue::from_str("unknown offset join")),
    })
}
fn options(join: JsValue, miter_limit: f64) -> Result<OffsetOptions, JsValue> {
    let (join, corner_joins) = if let Some(style) = join.as_string() {
        (join_style(&style)?, Vec::new())
    } else {
        (
            JoinStyle::Round,
            from_js::<Vec<String>>(join)?
                .iter()
                .map(|style| join_style(style))
                .collect::<Result<Vec<_>, _>>()?,
        )
    };
    Ok(OffsetOptions {
        join,
        corner_joins,
        miter_limit,
    })
}

#[wasm_bindgen]
pub struct NativeCurve2 {
    inner: TrimmedCurve2,
    retained: Rc<Curve2>,
    key: u32,
}
impl Drop for NativeCurve2 {
    fn drop(&mut self) {
        CURVES.with(|values| values.borrow_mut().remove(&self.key));
    }
}
impl NativeCurve2 {
    pub(super) fn wrap(inner: TrimmedCurve2) -> Self {
        let retained = Rc::new(inner.curve().clone());
        let key = NEXT_CURVE.with(|next| {
            let key = next.get() + 1;
            next.set(key);
            key
        });
        CURVES.with(|values| values.borrow_mut().insert(key, Rc::downgrade(&retained)));
        Self {
            inner,
            retained,
            key,
        }
    }
}
#[wasm_bindgen]
impl NativeCurve2 {
    #[wasm_bindgen(constructor)]
    pub fn new(value: JsValue) -> Result<Self, JsValue> {
        Ok(Self::wrap(from_js::<TrimmedCurveDto>(value)?.build()?))
    }
    #[wasm_bindgen(js_name=arcThrough)]
    pub fn arc_through(
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
    ) -> Result<Self, JsValue> {
        TrimmedCurve2::arc_through(
            Point2::new(x0, y0),
            Point2::new(x1, y1),
            Point2::new(x2, y2),
        )
        .map(Self::wrap)
        .map_err(js_error)
    }
    pub fn descriptor(&self) -> JsValue {
        let curve = match self.retained.as_ref() {
            Curve2::Line(c) => {
                serde_json::json!({"kind":"line","origin":[c.origin().x,c.origin().y],"direction":[c.direction().x,c.direction().y]})
            }
            Curve2::Circle(c) => {
                serde_json::json!({"kind":"circle","center":[c.center().x,c.center().y],"radius":c.radius()})
            }
            Curve2::Nurbs(c) => {
                serde_json::json!({"kind":"nurbs","degree":c.degree(),"control_points":c.control_points().iter().map(|p|[p.x,p.y]).collect::<Vec<_>>(),"weights":c.weights(),"knots":c.knots()})
            }
            Curve2::Offset(_) => serde_json::json!({"kind":"offset","key":self.key}),
            _ => serde_json::json!({"kind":"projected","key":self.key}),
        };
        json(
            serde_json::json!({"curve":curve,"range":{"start":self.inner.range().start,"end":self.inner.range().end}}),
        )
    }
    pub fn domain(&self) -> JsValue {
        let domain = self.inner.curve().domain();
        let bounds = match domain.bounds {
            shapex::kernel::DomainBounds::Bounded(range) => Some([range.start, range.end]),
            _ => None,
        };
        json(serde_json::json!({"bounds":bounds,"period":domain.period}))
    }
    pub fn evaluate(&self, u: f64) -> Result<Float64Array, JsValue> {
        let e = self.inner.curve().evaluate(u).map_err(js_error)?;
        Ok(f64_array(&[
            e.point.x,
            e.point.y,
            e.first_derivative.x,
            e.first_derivative.y,
            e.second_derivative.x,
            e.second_derivative.y,
        ]))
    }
    #[wasm_bindgen(js_name=closestPoint)]
    pub fn closest_point(&self, x: f64, y: f64, tol: &[f64]) -> Result<Float64Array, JsValue> {
        let p = self
            .inner
            .closest_point(Point2::new(x, y), tolerance(tol)?)
            .map_err(js_error)?;
        Ok(f64_array(&[p.parameter, p.point.x, p.point.y, p.distance]))
    }
    pub fn length(&self, tol: &[f64]) -> Result<f64, JsValue> {
        self.inner.length(tolerance(tol)?).map_err(js_error)
    }
    pub fn intersections(&self, other: &Self, tol: &[f64]) -> Result<JsValue, JsValue> {
        let hits = self
            .inner
            .intersections(&other.inner, tolerance(tol)?)
            .map_err(js_error)?;
        Ok(intersection_values(hits))
    }
    #[wasm_bindgen(js_name=selfIntersections)]
    pub fn self_intersections(&self, tol: &[f64]) -> Result<JsValue, JsValue> {
        Ok(intersection_values(
            self.inner
                .self_intersections(tolerance(tol)?)
                .map_err(js_error)?,
        ))
    }
    pub fn transformed(&self, angle: f64, x: f64, y: f64, tol: &[f64]) -> Result<Self, JsValue> {
        self.inner
            .transformed(
                shapex::kernel::RigidTransform2::new(angle, Vec2::new(x, y)).map_err(js_error)?,
                tolerance(tol)?,
            )
            .map(Self::wrap)
            .map_err(js_error)
    }
    #[wasm_bindgen(js_name=approximateNurbs)]
    pub fn approximate_nurbs(&self, tol: &[f64]) -> Result<JsValue, JsValue> {
        let approximation = self
            .inner
            .approximate_nurbs(tolerance(tol)?)
            .map_err(js_error)?;
        let result = Object::new();
        Reflect::set(
            &result,
            &"curve".into(),
            &Self::wrap(approximation.curve).into(),
        )?;
        Reflect::set(
            &result,
            &"estimatedError".into(),
            &approximation.estimated_error.into(),
        )?;
        Ok(result.into())
    }
    #[wasm_bindgen(js_name=splitAt)]
    pub fn split_at(&self, parameters: &[f64], tol: &[f64]) -> Result<Array, JsValue> {
        let result = Array::new();
        for curve in self
            .inner
            .split_at(parameters.iter().copied(), tolerance(tol)?)
            .map_err(js_error)?
        {
            result.push(&Self::wrap(curve).into());
        }
        Ok(result)
    }
    pub fn offset(&self, distance: f64) -> Result<Self, JsValue> {
        self.inner
            .offset(distance)
            .map(Self::wrap)
            .map_err(js_error)
    }
    pub fn tessellate(&self, linear: f64, angular: f64, depth: u8) -> Result<JsValue, JsValue> {
        let p = self
            .inner
            .tessellate(tessellation_options(linear, angular, depth)?)
            .map_err(js_error)?;
        Ok(json(
            serde_json::json!({"parameters":p.parameters,"points":p.points.iter().map(|p|[p.x,p.y]).collect::<Vec<_>>()}),
        ))
    }
    pub fn lift(&self, values: &[f64], tol: &[f64]) -> Result<NativeCurve3, JsValue> {
        let tol = tolerance(tol)?;
        Ok(NativeCurve3 {
            inner: plane(values, tol)?
                .lift_trimmed_curve(&self.inner, tol)
                .map_err(js_error)?,
            range: self.inner.range(),
        })
    }
}

#[wasm_bindgen(js_name=evaluateCurve2)]
pub fn evaluate_curve2(value: JsValue, u: f64) -> Result<Float64Array, JsValue> {
    let e = from_js::<CurveDto>(value)?
        .build()?
        .evaluate(u)
        .map_err(js_error)?;
    Ok(f64_array(&[
        e.point.x,
        e.point.y,
        e.first_derivative.x,
        e.first_derivative.y,
        e.second_derivative.x,
        e.second_derivative.y,
    ]))
}

#[wasm_bindgen(js_name=validateCurve2)]
pub fn validate_curve2(value: JsValue) -> Result<(), JsValue> {
    from_js::<CurveDto>(value)?.build().map(|_| ())
}

#[wasm_bindgen(js_name=projectCurve2)]
pub fn project_curve2(
    value: JsValue,
    x: f64,
    y: f64,
    tol: &[f64],
) -> Result<Float64Array, JsValue> {
    let projection = from_js::<CurveDto>(value)?
        .build()?
        .closest_point(Point2::new(x, y), tolerance(tol)?)
        .map_err(js_error)?;
    Ok(f64_array(&[
        projection.parameter,
        projection.point.x,
        projection.point.y,
        projection.distance,
    ]))
}

#[wasm_bindgen(js_name=classifyWire2)]
pub fn classify_wire(value: JsValue, x: f64, y: f64) -> Result<String, JsValue> {
    let dto = from_js::<WireDto>(value)?;
    let tol = dto.tolerance.into();
    dto.build()?
        .classify_point(Point2::new(x, y), tol)
        .map(classify)
        .map_err(js_error)
}
#[wasm_bindgen(js_name=classifyProfile2)]
pub fn classify_profile(value: JsValue, x: f64, y: f64) -> Result<String, JsValue> {
    profile(value)?
        .classify_point(Point2::new(x, y))
        .map(classify)
        .map_err(js_error)
}

/// Build a path from its transport form, with the tolerance it was created with.
fn path(value: JsValue, closed: bool) -> Result<(Path2, shapex::kernel::ModelTolerance), JsValue> {
    let dto = from_js::<WireDto>(value)?;
    let tol = dto.tolerance.into();
    let path = Path2::new(
        dto.segments
            .into_iter()
            .map(TrimmedCurveDto::build)
            .collect::<Result<Vec<_>, _>>()?,
        closed,
        tol,
    )
    .map_err(js_error)?;
    Ok((path, tol))
}

#[wasm_bindgen(js_name=offsetPath2)]
pub fn offset_path(
    value: JsValue,
    closed: bool,
    distance: f64,
    join: JsValue,
    miter: f64,
) -> Result<JsValue, JsValue> {
    let (path, tol) = path(value, closed)?;
    let result = path
        .offset(distance, options(join, miter)?, tol)
        .map_err(js_error)?;
    let curves = Array::new();
    for curve in result.path.segments() {
        curves.push(&NativeCurve2::wrap(curve.clone()).into());
    }
    let object = Object::new();
    Reflect::set(&object, &"curves".into(), &curves)?;
    Reflect::set(
        &object,
        &"sources".into(),
        &json(serde_json::json!(result.sources)),
    )?;
    Ok(object.into())
}

#[wasm_bindgen(js_name=validatePath2)]
pub fn validate_path(value: JsValue, closed: bool) -> Result<(), JsValue> {
    path(value, closed).map(|_| ())
}

#[wasm_bindgen(js_name=validateSimplePath2)]
pub fn validate_simple_path(value: JsValue, closed: bool) -> Result<(), JsValue> {
    let (path, tol) = path(value, closed)?;
    path.validate_simple(tol).map_err(js_error)
}

#[wasm_bindgen(js_name=pathSignedArea2)]
pub fn path_signed_area(value: JsValue) -> Result<f64, JsValue> {
    let (path, tol) = path(value, true)?;
    path.signed_area(tol).map_err(js_error)
}

#[wasm_bindgen(js_name=classifyPath2)]
pub fn classify_path(value: JsValue, x: f64, y: f64) -> Result<String, JsValue> {
    let (path, tol) = path(value, true)?;
    path.classify_point(Point2::new(x, y), tol)
        .map(classify)
        .map_err(js_error)
}
#[wasm_bindgen(js_name=offsetProfile2)]
pub fn offset_profile(value: JsValue, distance: f64) -> Result<Array, JsValue> {
    let result = Array::new();
    for profile in profile(value)?.offset(distance).map_err(js_error)? {
        let wires = Array::new();
        for wire in std::iter::once(profile.outer()).chain(profile.holes()) {
            let curves = Array::new();
            for curve in wire.segments() {
                curves.push(&NativeCurve2::wrap(curve.clone()).into());
            }
            wires.push(&curves);
        }
        result.push(&wires);
    }
    Ok(result)
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum Curve3Dto {
    Line {
        origin: [f64; 3],
        direction: [f64; 3],
    },
    Circle {
        center: [f64; 3],
        x_radius: [f64; 3],
        y_radius: [f64; 3],
    },
    Nurbs {
        degree: usize,
        control_points: Vec<[f64; 3]>,
        weights: Vec<f64>,
        knots: Vec<f64>,
    },
}
#[wasm_bindgen]
pub struct NativeCurve3 {
    pub(super) inner: Curve3,
    pub(super) range: ParameterRange,
}
#[wasm_bindgen]
impl NativeCurve3 {
    #[wasm_bindgen(constructor)]
    pub fn new(value: JsValue, start: f64, end: f64) -> Result<Self, JsValue> {
        let p = |p: [f64; 3]| Point3::new(p[0], p[1], p[2]);
        let v = |p: [f64; 3]| Vec3::new(p[0], p[1], p[2]);
        let inner: Curve3 = match from_js::<Curve3Dto>(value)? {
            Curve3Dto::Line { origin, direction } => {
                shapex::kernel::Line3::new(p(origin), v(direction)).map(Into::into)
            }
            Curve3Dto::Circle {
                center,
                x_radius,
                y_radius,
            } => shapex::kernel::Circle3::new(
                p(center),
                v(x_radius),
                v(y_radius),
                ModelTolerance::default(),
            )
            .map(Into::into),
            Curve3Dto::Nurbs {
                degree,
                control_points,
                weights,
                knots,
            } => shapex::kernel::NurbsCurve3::new(
                degree,
                control_points.into_iter().map(p).collect(),
                weights,
                knots,
            )
            .map(Into::into),
        }
        .map_err(js_error)?;
        let range = ParameterRange::new(start, end).map_err(js_error)?;
        if !inner
            .domain()
            .contains_range(range, ModelTolerance::default().parameter)
        {
            return Err(JsValue::from_str(
                "spatial trim lies outside the curve domain",
            ));
        }
        Ok(Self { inner, range })
    }
    pub fn range(&self) -> Float64Array {
        f64_array(&[self.range.start, self.range.end])
    }
    pub fn trimmed(&self, start: f64, end: f64) -> Result<Self, JsValue> {
        let range = ParameterRange::new(start, end).map_err(js_error)?;
        if !self
            .inner
            .domain()
            .contains_range(range, ModelTolerance::default().parameter)
        {
            return Err(JsValue::from_str("spatial trim lies outside curve domain"));
        }
        Ok(Self {
            inner: self.inner.clone(),
            range,
        })
    }
    pub fn evaluate(&self, u: f64) -> Result<Float64Array, JsValue> {
        let e = self.inner.evaluate(u).map_err(js_error)?;
        Ok(f64_array(&[
            e.point.x,
            e.point.y,
            e.point.z,
            e.first_derivative.x,
            e.first_derivative.y,
            e.first_derivative.z,
            e.second_derivative.x,
            e.second_derivative.y,
            e.second_derivative.z,
        ]))
    }
    pub fn transformed(&self, values: &[f64], tol: &[f64]) -> Result<Self, JsValue> {
        Ok(Self {
            inner: self
                .inner
                .transformed(transform(values, tolerance(tol)?)?, tolerance(tol)?)
                .map_err(js_error)?,
            range: self.range,
        })
    }
    pub fn project(&self, values: &[f64], tol: &[f64]) -> Result<JsValue, JsValue> {
        let tol = tolerance(tol)?;
        match plane(values, tol)?
            .project_curve(&self.inner, self.range, tol)
            .map_err(js_error)?
        {
            PlaneCurveProjection::Point(point) => Ok(json(
                serde_json::json!({"kind":"point","point":[point.x,point.y]}),
            )),
            PlaneCurveProjection::Curve(curve) => {
                let result = Object::new();
                Reflect::set(&result, &"kind".into(), &"curve".into())?;
                Reflect::set(&result, &"curve".into(), &NativeCurve2::wrap(curve).into())?;
                Ok(result.into())
            }
        }
    }
    #[wasm_bindgen(js_name=expressedIn)]
    pub fn expressed_in(&self, values: &[f64], tol: &[f64]) -> Result<NativeCurve2, JsValue> {
        let tol = tolerance(tol)?;
        plane(values, tol)?
            .express_curve(&self.inner, self.range, tol)
            .map(NativeCurve2::wrap)
            .map_err(js_error)
    }
    #[wasm_bindgen(js_name=closestPoint)]
    pub fn closest_point(
        &self,
        x: f64,
        y: f64,
        z: f64,
        tol: &[f64],
    ) -> Result<Float64Array, JsValue> {
        let p = self
            .inner
            .closest_point(Point3::new(x, y, z), self.range, tolerance(tol)?)
            .map_err(js_error)?;
        Ok(f64_array(&[
            p.parameter,
            p.point.x,
            p.point.y,
            p.point.z,
            p.distance,
        ]))
    }
}
