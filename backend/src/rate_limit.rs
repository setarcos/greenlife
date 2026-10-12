use std::net::{IpAddr, Ipv6Addr};

use actix_governor::{KeyExtractor, SimpleKeyExtractionError};
use actix_web::dev::ServiceRequest;

/// 登录限流用的 key 提取器：尽量取真实客户端 IP，而不是 TCP 对端地址。
///
/// actix-governor 默认的 `PeerIpKeyExtractor` 读 `req.peer_addr()`，在本项目的
/// 部署形态下（nginx 反代到 127.0.0.1:8080，见 README）永远是 127.0.0.1，
/// 限流会退化成「全局限流」。
///
/// 这里做两件事：
/// 1. 只有直连对端是回环地址时才采信 `X-Forwarded-For`。其他来源说明请求没
///    经过本机代理，头里的内容可能是客户端伪造的，采信等于让人随便换 IP 绕过限流；
/// 2. 取 XFF 的**最后一段**。nginx 里配了
///    `proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;`（README 的
///    nginx 配置里有）时，它会把自己的 `$remote_addr` 追加到末尾，客户端自带的
///    伪造值只会排在更前面。nginx 配了 realip 模块时（Cloudflare Tunnel 场景，
///    见 README），这一段就是真实客户端 IP。
///
/// 注意：nginx 默认只改 `Host` 和 `Connection`，`X-Forwarded-For` 是原样透传的。
/// 所以反向代理那边必须显式设置这个头，否则这里会把客户端自报的 IP 当真。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RealIpKeyExtractor;

impl KeyExtractor for RealIpKeyExtractor {
    type Key = IpAddr;
    type KeyExtractionError = SimpleKeyExtractionError<&'static str>;

    fn extract(&self, req: &ServiceRequest) -> Result<Self::Key, Self::KeyExtractionError> {
        let peer = req.peer_addr().map(|socket| socket.ip()).ok_or_else(|| {
            SimpleKeyExtractionError::new("Could not extract peer IP address from request")
        })?;

        // 头部缺失或内容解析不了时回落到对端地址，不因为一个坏头就拒绝请求。
        let ip = if peer.is_loopback() {
            req.headers()
                .get("x-forwarded-for")
                .and_then(|value| value.to_str().ok())
                .and_then(client_ip_from_xff)
                .unwrap_or(peer)
        } else {
            peer
        };

        Ok(rate_limit_key(ip))
    }
}

/// 取 `X-Forwarded-For` 的最后一段并解析成 IP。
/// nginx 追加在末尾的那段才是本机代理认定的客户端地址；解析不了
/// （空值、垃圾内容、`ip:port` 之类的写法）就返回 `None`。
fn client_ip_from_xff(value: &str) -> Option<IpAddr> {
    value.rsplit(',').next()?.trim().parse().ok()
}

/// IPv6 邻居往往成批拿到同一个 /56 前缀，临时地址又会在其中轮换，
/// 所以按 /56 归一化，避免换个地址就重置额度（与 actix-governor 默认提取器一致）。
fn rate_limit_key(mut ip: IpAddr) -> IpAddr {
    if let IpAddr::V6(v6) = ip {
        let mut octets = v6.octets();
        octets[7..16].fill(0);
        ip = IpAddr::V6(Ipv6Addr::from(octets));
    }
    ip
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::test::TestRequest;
    use std::net::SocketAddr;

    fn extract(peer: &str, xff: Option<&str>) -> IpAddr {
        let peer: SocketAddr = peer.parse().unwrap();
        let mut req = TestRequest::default().peer_addr(peer);
        if let Some(value) = xff {
            req = req.insert_header(("x-forwarded-for", value));
        }
        RealIpKeyExtractor.extract(&req.to_srv_request()).unwrap()
    }

    #[test]
    fn loopback_peer_uses_forwarded_ip() {
        assert_eq!(
            extract("127.0.0.1:5321", Some("203.0.113.7")),
            "203.0.113.7".parse::<IpAddr>().unwrap()
        );
        assert_eq!(
            extract("[::1]:5321", Some("203.0.113.7")),
            "203.0.113.7".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn forwarded_takes_last_entry() {
        // 前面几段可能是客户端自己塞的，nginx 追加的才是最后一段
        assert_eq!(
            extract("127.0.0.1:5321", Some("1.2.3.4, 203.0.113.7")),
            "203.0.113.7".parse::<IpAddr>().unwrap()
        );
        // Cloudflare Tunnel 下 nginx 的 realip 不做递归，重复也取末尾
        assert_eq!(
            extract("127.0.0.1:5321", Some("203.0.113.7, 203.0.113.7")),
            "203.0.113.7".parse::<IpAddr>().unwrap()
        );
        // 末尾带空格
        assert_eq!(
            extract("127.0.0.1:5321", Some("10.0.0.1 ,198.51.100.9")),
            "198.51.100.9".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn forwarded_falls_back_to_peer() {
        // 无头 / 空头 / 垃圾 / 带端口，都用对端地址
        assert_eq!(
            extract("127.0.0.1:5321", None),
            "127.0.0.1".parse::<IpAddr>().unwrap()
        );
        assert_eq!(
            extract("127.0.0.1:5321", Some("")),
            "127.0.0.1".parse::<IpAddr>().unwrap()
        );
        assert_eq!(
            extract("127.0.0.1:5321", Some("unknown")),
            "127.0.0.1".parse::<IpAddr>().unwrap()
        );
        assert_eq!(
            extract("127.0.0.1:5321", Some("203.0.113.7:8080")),
            "127.0.0.1".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn untrusted_peer_ignores_forwarded_ip() {
        // 非回环对端说明没走本机代理，XFF 可能是伪造的，必须忽略
        assert_eq!(
            extract("198.51.100.9:5321", Some("203.0.113.7")),
            "198.51.100.9".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn ipv6_is_grouped_by_56_prefix() {
        let a = extract("127.0.0.1:5321", Some("2001:db8:1234:5600::1"));
        let b = extract("127.0.0.1:5321", Some("2001:db8:1234:56ff::9999"));
        assert_eq!(a, b);
        assert_eq!(a, "2001:db8:1234:5600::".parse::<IpAddr>().unwrap());
        // 邻居的前缀不同，不该被合并
        assert_ne!(a, extract("127.0.0.1:5321", Some("2001:db8:1234:5700::1")));
    }
}
