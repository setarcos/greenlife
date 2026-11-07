use std::net::{IpAddr, Ipv6Addr};
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use actix_web::body::{EitherBody, MessageBody};
use actix_web::dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform};
use actix_web::{Error as ActixWebError, HttpResponse};
use futures_util::future::{ok, ready, FutureExt, LocalBoxFuture};
use governor::clock::{Clock, DefaultClock};
use governor::state::keyed::DefaultKeyedStateStore;
use governor::{Quota, RateLimiter};

use crate::errors::ServiceError;

/// 限流用的 key 就是客户端 IP（IPv6 已按 /56 归一化，见 [`rate_limit_key`]）。
type Key = IpAddr;

type Limiter = RateLimiter<Key, DefaultKeyedStateStore<Key>, DefaultClock>;

/// 按客户端 IP 限流的中间件。
///
/// 为什么不直接用 `actix-governor`：它只是在 `governor` 外面包了一层中间件，
/// 自身却是 GPL-3.0-or-later，链接进二进制后整个项目就没法再用宽松协议发布。
/// `governor` 本身是 MIT，这里直接用它，中间件只有几十行。
///
/// 一个实例就是一个**全进程共享**的限流器，所以必须在 `HttpServer::new`
/// 之外构造。在闭包内构造会让每个 worker 各持一份，实际额度变成
/// worker 数 × burst。
#[derive(Clone)]
pub struct RateLimit {
    limiter: Arc<Limiter>,
    clock: DefaultClock,
}

impl RateLimit {
    /// 每个 key 允许 `burst` 次突发，之后每 `period` 补充一次。
    pub fn new(burst: u32, period: Duration) -> Self {
        let quota = Quota::with_period(period)
            .expect("rate limit period must be non-zero")
            .allow_burst(NonZeroU32::new(burst).expect("burst size must be non-zero"));

        Self {
            limiter: Arc::new(RateLimiter::keyed(quota)),
            clock: DefaultClock::default(),
        }
    }
}

/// 取限流用的 key：尽量取真实客户端 IP，而不是 TCP 对端地址。
///
/// `governor` 的默认做法只能看 `req.peer_addr()`，在本项目的部署形态下
/// （nginx 反代到 127.0.0.1:8080，见 README）永远是 127.0.0.1，
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
pub fn client_key(req: &ServiceRequest) -> Result<Key, ServiceError> {
    let peer = req.peer_addr().map(|socket| socket.ip()).ok_or_else(|| {
        log::error!("could not extract peer IP address from request");
        ServiceError::InternalServerError
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

/// 取 `X-Forwarded-For` 的最后一段并解析成 IP。
/// nginx 追加在末尾的那段才是本机代理认定的客户端地址；解析不了
/// （空值、垃圾内容、`ip:port` 之类的写法）就返回 `None`。
fn client_ip_from_xff(value: &str) -> Option<IpAddr> {
    value.rsplit(',').next()?.trim().parse().ok()
}

/// IPv6 邻居往往成批拿到同一个 /56 前缀，临时地址又会在其中轮换，
/// 所以按 /56 归一化，避免换个地址就重置额度。
fn rate_limit_key(mut ip: IpAddr) -> IpAddr {
    if let IpAddr::V6(v6) = ip {
        let mut octets = v6.octets();
        octets[7..16].fill(0);
        ip = IpAddr::V6(Ipv6Addr::from(octets));
    }
    ip
}

impl<S, B> Transform<S, ServiceRequest> for RateLimit
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = ActixWebError> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = ActixWebError;
    type InitError = ();
    type Transform = RateLimitMiddleware<S>;
    type Future = LocalBoxFuture<'static, Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(RateLimitMiddleware {
            service: Rc::new(service),
            limiter: Arc::clone(&self.limiter),
            clock: self.clock.clone(),
        })
        .boxed_local()
    }
}

pub struct RateLimitMiddleware<S> {
    service: Rc<S>,
    limiter: Arc<Limiter>,
    clock: DefaultClock,
}

impl<S, B> Service<ServiceRequest> for RateLimitMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = ActixWebError> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = ActixWebError;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let key = match client_key(&req) {
            Ok(key) => key,
            Err(err) => return ready(Err(err.into())).boxed_local(),
        };

        // 超限时直接返回 429 响应，而不是 Err：Logger 只对正常返回的响应记
        // 访问日志，返回 Err 的请求在日志里会整条消失。
        match self.limiter.check_key(&key) {
            Ok(()) => {
                // 用 Rc 而不是直接借用 self：call() 返回的 future 必须是 'static，
                // 而 await 下游之前还要把 req 移进去。
                let service = Rc::clone(&self.service);
                Box::pin(async move {
                    let res = service.call(req).await?;
                    Ok(res.map_into_left_body())
                })
                .boxed_local()
            }
            Err(not_until) => {
                let wait = not_until.wait_time_from(Clock::now(&self.clock));
                // 向上取整：1.2s 写成 2 秒，别让客户端提前重试
                let retry_after = wait.as_secs() + u64::from(wait.subsec_nanos() > 0);
                log::warn!("rate limit exceeded for {key}, retry after {retry_after}s");

                let response = HttpResponse::TooManyRequests()
                    .insert_header(("retry-after", retry_after.to_string()))
                    .json(serde_json::json!({
                        "error": "Too many requests, please try again later"
                    }))
                    .map_into_right_body();

                ready(Ok(req.into_response(response))).boxed_local()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::http::StatusCode;
    use actix_web::test::{self, TestRequest};
    use actix_web::{web, App, HttpResponse};
    use std::net::SocketAddr;

    fn extract(peer: &str, xff: Option<&str>) -> IpAddr {
        let peer: SocketAddr = peer.parse().unwrap();
        let mut req = TestRequest::default().peer_addr(peer);
        if let Some(value) = xff {
            req = req.insert_header(("x-forwarded-for", value));
        }
        client_key(&req.to_srv_request()).unwrap()
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

    fn login_req(peer: &str, xff: Option<&str>) -> TestRequest {
        let req = TestRequest::get()
            .uri("/auth/login")
            .peer_addr(peer.parse().unwrap());
        match xff {
            Some(value) => req.insert_header(("x-forwarded-for", value)),
            None => req,
        }
    }

    /// 每个测试都要一份新的限流器；`init_service` 返回的 `impl Service<...>`
    /// 没法写进函数签名，所以用宏展开。
    macro_rules! test_app {
        () => {
            test::init_service(
                App::new().service(
                    web::scope("/auth")
                        .wrap(RateLimit::new(5, Duration::from_secs(2)))
                        .route(
                            "/login",
                            web::get().to(|| async { HttpResponse::Ok().finish() }),
                        ),
                ),
            )
            .await
        };
    }

    #[actix_web::test]
    async fn burst_is_limited_then_rejects() {
        let app = test_app!();

        for i in 0..5 {
            assert_eq!(
                test::call_service(&app, login_req("198.51.100.9:5321", None).to_request())
                    .await
                    .status(),
                StatusCode::OK,
                "request {} should be allowed",
                i + 1
            );
        }
        let limited =
            test::call_service(&app, login_req("198.51.100.9:5321", None).to_request()).await;
        assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
        // 告诉客户端多久之后可以重试，否则只能盲等
        assert!(limited.headers().contains_key("retry-after"));
        let body: serde_json::Value = test::read_body_json(limited).await;
        assert!(body["error"]
            .as_str()
            .unwrap()
            .contains("Too many requests"));
    }

    #[actix_web::test]
    async fn clients_have_independent_buckets() {
        let app = test_app!();

        for _ in 0..5 {
            assert_eq!(
                test::call_service(&app, login_req("198.51.100.9:5321", None).to_request())
                    .await
                    .status(),
                StatusCode::OK
            );
        }
        assert_eq!(
            test::call_service(&app, login_req("198.51.100.9:5321", None).to_request())
                .await
                .status(),
            StatusCode::TOO_MANY_REQUESTS
        );

        // 另一个客户端不受影响
        assert_eq!(
            test::call_service(&app, login_req("198.51.100.10:5321", None).to_request())
                .await
                .status(),
            StatusCode::OK
        );
    }

    #[actix_web::test]
    async fn forwarded_ip_is_the_bucket_key() {
        let app = test_app!();

        // 反代后面所有请求的对端都是回环地址，真正区分客户端的是 XFF
        for _ in 0..5 {
            assert_eq!(
                test::call_service(
                    &app,
                    login_req("127.0.0.1:5321", Some("203.0.113.7")).to_request()
                )
                .await
                .status(),
                StatusCode::OK
            );
        }
        assert_eq!(
            test::call_service(
                &app,
                login_req("127.0.0.1:5321", Some("203.0.113.7")).to_request()
            )
            .await
            .status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(
            test::call_service(
                &app,
                login_req("127.0.0.1:5321", Some("203.0.113.8")).to_request()
            )
            .await
            .status(),
            StatusCode::OK
        );
    }
}
