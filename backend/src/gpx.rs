//! 鸟调 GPX 的解析：从 Locus Map 导出的文件里取出每个 waypoint。
//!
//! 一次鸟调导出的 GPX 里，一个 `<wpt>` 就是一条鸟种观察：
//!
//! ```xml
//! <wpt lat="39.993376" lon="116.298884">
//!   <ele>43.51</ele>
//!   <time>2018-05-22T00:08:19.046Z</time>
//!   <name>普通楼燕</name>
//!   <desc><![CDATA[3]]></desc>
//!   <extensions><gpxx:WaypointExtension><gpxx:Address>
//!     <gpxx:StreetAddress>幼鸟</gpxx:StreetAddress>
//!   </gpxx:Address></gpxx:WaypointExtension></extensions>
//! </wpt>
//! ```
//!
//! 只认这几项：`name`（鸟种中文名）、`desc`（数量）、`time`（时间）、
//! `StreetAddress`（备注）、`lat` / `lon`（坐标）。轨迹 `<trk>` 不关心。
//!
//! 用流式事件解析而不是 serde：GPX 里带命名空间的扩展（`gpxx:`）用 serde
//! 反序列化要写一堆 rename，事件解析反而更短也更抗格式漂移。

use chrono::{DateTime, Duration, NaiveDateTime};
use quick_xml::events::Event;
use quick_xml::Reader;

/// GPX 里的一个 waypoint。缺字段一律为 `None`，由调用方决定怎么兜底。
#[derive(Debug, Default, Clone, PartialEq)]
pub struct GpxWaypoint {
    /// `<name>`：鸟种中文名。
    pub name: Option<String>,
    /// `<desc>`：观察到的个体数（文本，可能是空的）。
    pub description: Option<String>,
    /// `gpxx:StreetAddress`：备注（幼鸟 / 1雌带5幼 / 雄…）。
    pub remark: Option<String>,
    /// `<time>`：UTC，已换算成北京时间。
    pub time: Option<NaiveDateTime>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

impl GpxWaypoint {
    /// `<desc>` 是数量时解析成整数；不是数字（或为空）时返回 `None`。
    pub fn quantity(&self) -> Option<i32> {
        self.description
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .and_then(|text| text.parse::<i32>().ok())
    }
}

/// 解析整份 GPX，按出现顺序返回所有 waypoint。
pub fn parse_waypoints(xml: &str) -> Result<Vec<GpxWaypoint>, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut waypoints = Vec::new();
    // 当前正在读的 waypoint；`None` = 不在 `<wpt>` 里（metadata / trk 里的同名标签要忽略）。
    let mut current: Option<GpxWaypoint> = None;
    // 当前正在累积文本的字段。
    let mut field: Option<Field> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event)) => {
                match event.local_name().as_ref() {
                    b"wpt" => {
                        let mut waypoint = GpxWaypoint::default();
                        for attribute in event.attributes().flatten() {
                            let value = String::from_utf8_lossy(&attribute.value).into_owned();
                            match attribute.key.as_ref() {
                                b"lat" => waypoint.latitude = value.parse().ok(),
                                b"lon" => waypoint.longitude = value.parse().ok(),
                                _ => {}
                            }
                        }
                        current = Some(waypoint);
                        field = None;
                    }
                    b"name" if current.is_some() => field = Some(Field::Name),
                    b"desc" if current.is_some() => field = Some(Field::Description),
                    b"time" if current.is_some() => field = Some(Field::Time),
                    b"StreetAddress" if current.is_some() => field = Some(Field::Remark),
                    _ => {}
                }
            }
            Ok(Event::Text(event)) => {
                if let (Some(waypoint), Some(field)) = (current.as_mut(), field) {
                    let text = event
                        .decode()
                        .map_err(|e| format!("invalid text content: {e}"))?;
                    set_field(waypoint, field, &text);
                }
            }
            Ok(Event::CData(event)) => {
                if let (Some(waypoint), Some(field)) = (current.as_mut(), field) {
                    let text = event
                        .decode()
                        .map_err(|e| format!("invalid CDATA content: {e}"))?;
                    set_field(waypoint, field, &text);
                }
            }
            Ok(Event::End(event)) => match event.local_name().as_ref() {
                b"wpt" => {
                    if let Some(waypoint) = current.take() {
                        waypoints.push(waypoint);
                    }
                    field = None;
                }
                b"name" | b"desc" | b"time" | b"StreetAddress" => field = None,
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("invalid GPX at position {}: {e}", reader.buffer_position())),
            _ => {}
        }
    }

    Ok(waypoints)
}

/// 文本可能是分几次事件到达的（实体、换行），追加而不是覆盖。
fn set_field(waypoint: &mut GpxWaypoint, field: Field, text: &str) {
    let slot = match field {
        Field::Name => &mut waypoint.name,
        Field::Description => &mut waypoint.description,
        Field::Time => {
            if waypoint.time.is_none() {
                waypoint.time = parse_time(text);
            }
            return;
        }
        Field::Remark => &mut waypoint.remark,
    };
    match slot {
        Some(existing) => existing.push_str(text),
        None => *slot = Some(text.to_string()),
    }
}

/// GPX 的 `<time>` 是 RFC 3339 的 UTC（`2018-05-22T00:08:19.046Z`）。
/// 鸟调都在北京，存库前换算成北京时间，界面上的「时间」才是观察者看到的钟点。
fn parse_time(raw: &str) -> Option<NaiveDateTime> {
    let utc: DateTime<chrono::FixedOffset> = DateTime::parse_from_rfc3339(raw.trim()).ok()?;
    Some(utc.naive_utc() + Duration::hours(8))
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Field {
    Name,
    Description,
    Time,
    Remark,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<gpx version="1.1" xmlns="http://www.topografix.com/GPX/1/1" xmlns:gpxx="http://www.garmin.com/xmlschemas/GpxExtensions/v3">
	<metadata><desc>File with points/tracks from Locus Map</desc></metadata>
	<wpt lat="39.993376" lon="116.298884">
		<ele>43.51</ele>
		<time>2018-05-22T00:08:19.046Z</time>
		<name>普通楼燕</name>
		<desc><![CDATA[3]]></desc>
		<pdop>3.22</pdop>
	</wpt>
	<wpt lat="39.992291" lon="116.298782">
		<time>2018-05-22T00:31:30.560Z</time>
		<name>四声杜鹃</name>
		<desc><![CDATA[1]]></desc>
		<extensions>
			<gpxx:WaypointExtension>
				<gpxx:Address>
					<gpxx:StreetAddress>1雌带5幼</gpxx:StreetAddress>
				</gpxx:Address>
			</gpxx:WaypointExtension>
		</extensions>
	</wpt>
</gpx>"#;

    #[test]
    fn parses_waypoints_in_order() {
        let waypoints = parse_waypoints(SAMPLE).unwrap();
        assert_eq!(waypoints.len(), 2);

        let first = &waypoints[0];
        assert_eq!(first.name.as_deref(), Some("普通楼燕"));
        assert_eq!(first.quantity(), Some(3));
        assert_eq!(first.latitude, Some(39.993376));
        assert_eq!(first.longitude, Some(116.298884));
        assert_eq!(first.remark, None);
        // 00:08:19 UTC = 08:08:19 北京时间
        assert_eq!(
            first.time,
            NaiveDate::from_ymd_opt(2018, 5, 22).unwrap().and_hms_milli_opt(8, 8, 19, 46)
        );

        let second = &waypoints[1];
        assert_eq!(second.name.as_deref(), Some("四声杜鹃"));
        assert_eq!(second.remark.as_deref(), Some("1雌带5幼"));
    }

    #[test]
    fn metadata_name_is_not_a_waypoint() {
        let xml = r#"<gpx><metadata><name>index</name></metadata><wpt lat="1" lon="2"><name>麻雀</name></wpt></gpx>"#;
        let waypoints = parse_waypoints(xml).unwrap();
        assert_eq!(waypoints.len(), 1);
        assert_eq!(waypoints[0].name.as_deref(), Some("麻雀"));
    }

    #[test]
    fn non_numeric_description_has_no_quantity() {
        let xml = r#"<gpx><wpt lat="1" lon="2"><name>麻雀</name><desc>多只</desc></wpt></gpx>"#;
        let waypoints = parse_waypoints(xml).unwrap();
        assert_eq!(waypoints[0].quantity(), None);
        assert_eq!(waypoints[0].description.as_deref(), Some("多只"));
    }

    #[test]
    fn rejects_malformed_xml() {
        assert!(parse_waypoints("<gpx><wpt></gpx>").is_err());
    }

    /// 用仓库根目录的真实样本文件跑一遍（文件不在时跳过，例如 CI）。
    #[test]
    fn parses_the_reference_file_when_present() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../20180522北大鸟调多云转晴.gpx"
        );
        let Ok(xml) = std::fs::read_to_string(path) else {
            return;
        };
        let waypoints = parse_waypoints(&xml).unwrap();
        assert_eq!(waypoints.len(), 98);
        assert_eq!(waypoints.iter().filter(|w| w.remark.is_some()).count(), 8);
        // 每个 waypoint 都有中文名。
        assert!(waypoints.iter().all(|w| w.name.is_some()));
    }
}
