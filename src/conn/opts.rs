#[cfg(any(feature = "socket", feature = "pipe"))]
use std::net::{Ipv4Addr, Ipv6Addr};

#[cfg(any(feature = "socket", feature = "ssl"))]
use std::path;

#[cfg(any(feature = "socket", feature = "pipe"))]
use std::str::FromStr;

use super::super::error::UrlError;

use url::{
    UrlParser,
    SchemeType,
};

/// Mysql connection options.
///
/// For example:
///
/// ```ignore
/// let opts = Opts {
///     user: Some("username".to_string()),
///     pass: Some("password".to_string()),
///     db_name: Some("mydatabase".to_string()),
///     ..Default::default()
/// };
/// ```
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Opts {
    /// Address of mysql server (defaults to `127.0.0.1`). Hostnames should also work.
    pub ip_or_hostname: Option<String>,
    /// TCP port of mysql server (defaults to `3306`).
    pub tcp_port: u16,
    /// Path to unix socket of mysql server (defaults to `None`).
    #[cfg(feature = "socket")]
    pub unix_addr: Option<path::PathBuf>,
    /// Pipe name of mysql server (defaults to `None`).
    #[cfg(feature = "pipe")]
    pub pipe_name: Option<String>,
    /// User (defaults to `None`).
    pub user: Option<String>,
    /// Password (defaults to `None`).
    pub pass: Option<String>,
    /// Database name (defaults to `None`).
    pub db_name: Option<String>,

    #[cfg(any(feature = "socket", feature = "pipe"))]
    /// Prefer socket connection (defaults to `true`).
    ///
    /// Will reconnect via socket after TCP connection to `127.0.0.1` if `true`.
    pub prefer_socket: bool,
    // XXX: Wait for keepalive_timeout stabilization
    /// Commands to execute on each new database connection.
    pub init: Vec<String>,

    #[cfg(feature = "ssl")]
    /// #### Only available if `ssl` feature enabled.
    /// Perform or not ssl peer verification (defaults to `false`).
    /// Only make sense if ssl_opts is not None.
    pub verify_peer: bool,

    #[cfg(feature = "ssl")]
    /// #### Only available if `ssl` feature enabled.
    /// SSL certificates and keys in pem format.
    /// If not None, then ssl connection implied.
    ///
    /// `Option<(ca_cert, Option<(client_cert, client_key)>)>.`
    pub ssl_opts: Option<(path::PathBuf, Option<(path::PathBuf, path::PathBuf)>)>
}

impl Opts {
    #[doc(hidden)]
    #[cfg(any(feature = "socket", feature = "pipe"))]
    pub fn addr_is_loopback(&self) -> bool {
        if self.ip_or_hostname.is_some() {
            let v4addr: Option<Ipv4Addr> = FromStr::from_str(
                self.ip_or_hostname.as_ref().unwrap().as_ref()).ok();
            let v6addr: Option<Ipv6Addr> = FromStr::from_str(
                self.ip_or_hostname.as_ref().unwrap().as_ref()).ok();
            if let Some(addr) = v4addr {
                addr.is_loopback()
            } else if let Some(addr) = v6addr {
                addr.is_loopback()
            } else if self.ip_or_hostname.as_ref().unwrap() == "localhost" {
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    pub fn from_url(url: &str) -> Result<Opts, UrlError> {
        from_url(url)
    }

    #[cfg(any(feature = "socket", feature = "pipe"))]
    fn set_prefer_socket(&mut self, val: bool) {
        self.prefer_socket = val;
    }

    #[allow(unused_variables)]
    #[cfg(all(not(feature = "socket"), not(feature = "pipe")))]
    fn set_prefer_socket(&mut self, val: bool) {
        ()
    }

    #[cfg(feature = "ssl")]
    fn set_verify_peer(&mut self, val: bool) {
        self.verify_peer = val;
    }

    #[allow(unused_variables)]
    #[cfg(not(feature = "ssl"))]
    fn set_verify_peer(&mut self, val: bool) {
        ()
    }
}

#[cfg(all(not(feature = "ssl"), feature = "socket", not(feature = "pipe")))]
impl Default for Opts {
    fn default() -> Opts {
        Opts {
            ip_or_hostname: Some("127.0.0.1".to_string()),
            tcp_port: 3306,
            unix_addr: None,
            user: None,
            pass: None,
            db_name: None,
            prefer_socket: true,
            init: vec![],
        }
    }
}

#[cfg(all(not(feature = "ssl"), not(feature = "socket"), not(feature = "pipe")))]
impl Default for Opts {
    fn default() -> Opts {
        Opts {
            ip_or_hostname: Some("127.0.0.1".to_string()),
            tcp_port: 3306,
            user: None,
            pass: None,
            db_name: None,
            init: vec![],
        }
    }
}

#[cfg(all(not(feature = "ssl"), not(feature = "socket"), feature = "pipe"))]
impl Default for Opts {
    fn default() -> Opts {
        Opts {
            ip_or_hostname: Some("127.0.0.1".to_string()),
            tcp_port: 3306,
            pipe_name: None,
            user: None,
            pass: None,
            db_name: None,
            prefer_socket: true,
            init: vec![],
        }
    }
}

#[cfg(all(feature = "ssl", not(feature = "socket"), not(feature = "pipe")))]
impl Default for Opts {
    fn default() -> Opts {
        Opts {
            ip_or_hostname: Some("127.0.0.1".to_string()),
            tcp_port: 3306,
            user: None,
            pass: None,
            db_name: None,
            init: vec![],
            verify_peer: false,
            ssl_opts: None,
        }
    }
}

#[cfg(all(feature = "ssl", not(feature = "socket"), feature = "pipe"))]
impl Default for Opts {
    fn default() -> Opts {
        Opts {
            ip_or_hostname: Some("127.0.0.1".to_string()),
            tcp_port: 3306,
            pipe_name: None,
            user: None,
            pass: None,
            db_name: None,
            init: vec![],
            verify_peer: false,
            prefer_socket: true,
            ssl_opts: None,
        }
    }
}

#[cfg(all(feature = "ssl", feature = "socket", not(feature = "pipe")))]
impl Default for Opts {
    fn default() -> Opts {
        Opts {
            ip_or_hostname: Some("127.0.0.1".to_string()),
            tcp_port: 3306,
            unix_addr: None,
            user: None,
            pass: None,
            db_name: None,
            prefer_socket: true,
            init: vec![],
            verify_peer: false,
            ssl_opts: None,
        }
    }
}

fn from_url_basic(url: &str) -> Result<(Opts, Vec<(String, String)>), UrlError> {
    fn scheme_type_mapper(scheme: &str) -> SchemeType {
        match scheme {
            "mysql" => SchemeType::Relative(3306),
            _ => SchemeType::NonRelative,
        }
    }

    let mut parser = UrlParser::new();
    parser.scheme_type_mapper(scheme_type_mapper);
    let url = try!(parser.parse(url));
    if url.scheme != "mysql" {
        return Err(UrlError::UnsupportedScheme(url.scheme))
    }
    let user = url.lossy_percent_decode_username();
    let pass = url.lossy_percent_decode_password();
    let ip_or_hostname = match url.domain() {
        Some(domain) => Some(domain.to_string()),
        None => Some("127.0.0.1".to_string()),
    };
    let tcp_port = url.port().unwrap_or(3306);
    let db_name = match url.path() {
        Some(path) => {
            if path.len() > 0 {
                Some(path[0].clone())
            } else {
                None
            }
        },
        None => None,
    };
    let query_pairs = url.query_pairs().unwrap_or(Vec::new());
    let opts = Opts {
        user: user,
        pass: pass,
        ip_or_hostname: ip_or_hostname,
        tcp_port: tcp_port,
        db_name: db_name,
        ..Opts::default()
    };
    Ok((opts, query_pairs))
}

fn from_url(url: &str) -> Result<Opts, UrlError> {
    let (mut opts, query_pairs) = try!(from_url_basic(url));
    for (key, value) in query_pairs {
        if key == "prefer_socket" {
            if cfg!(all(not(feature = "socket"), not(feature = "pipe"))) {
                return Err(
                    UrlError::FeatureRequired("`socket' or `pipe'".into(), "prefer_socket".into())
                );
            } else {
                if value == "true" {
                    opts.set_prefer_socket(true);
                } else if value == "false" {
                    opts.set_prefer_socket(false);
                } else {
                    return Err(UrlError::InvalidValue("prefer_socket".into(), value));
                }
            }
        } else if key == "verify_peer" {
            if cfg!(not(feature = "ssl")) {
                return Err(UrlError::FeatureRequired("`ssl'".into(), "verify_peer".into()));
            } else {
                if value == "true" {
                    opts.set_verify_peer(true);
                } else if value == "false" {
                    opts.set_verify_peer(false);
                } else {
                    return Err(UrlError::InvalidValue("verify_peer".into(), value));
                }
            }
        } else {
            return Err(UrlError::UnknownParameter(key));
        }
    }
    Ok(opts)
}

impl<'a> From<&'a str> for Opts {
    fn from(url: &'a str) -> Opts {
        match from_url(url) {
            Ok(opts) => opts,
            Err(err) => panic!("{}", err),
        }
    }
}

#[cfg(test)]
mod test {
    use super::Opts;

    #[test]
    #[cfg(all(feature = "ssl", feature = "socket"))]
    fn should_convert_url_into_opts() {
        let opts = "mysql://usr:pw@localhost:3308/dbname?prefer_socket=false&verify_peer=true";
        assert_eq!(Opts {
            user: Some("usr".to_string()),
            pass: Some("pw".to_string()),
            ip_or_hostname: Some("localhost".to_string()),
            tcp_port: 3308,
            db_name: Some("dbname".to_string()),
            prefer_socket: false,
            verify_peer: true,
            ..Opts::default()
        }, opts.into());
    }

    #[test]
    #[cfg(all(not(feature = "ssl"), not(feature = "socket")))]
    fn should_convert_url_into_opts() {
        let opts = "mysql://usr:pw@localhost:3308/dbname";
        assert_eq!(Opts {
            user: Some("usr".to_string()),
            pass: Some("pw".to_string()),
            ip_or_hostname: Some("localhost".to_string()),
            tcp_port: 3308,
            db_name: Some("dbname".to_string()),
            ..Opts::default()
        }, opts.into());
    }

    #[test]
    #[should_panic]
    fn should_panic_on_invalid_url() {
        let opts = "42";
        let _: Opts = opts.into();
    }

    #[test]
    #[should_panic]
    fn should_panic_on_invalid_scheme() {
        let opts = "postgres://localhost";
        let _: Opts = opts.into();
    }

    #[test]
    #[should_panic]
    fn should_panic_on_unknown_query_param() {
        let opts = "mysql://localhost/foo?bar=baz";
        let _: Opts = opts.into();
    }

    #[test]
    #[should_panic]
    #[cfg(not(feature = "socket"))]
    fn should_panic_if_prefer_socket_query_param_requires_feature() {
        let opts = "mysql://usr:pw@localhost:3308/dbname?prefer_socket=false";
        let _: Opts = opts.into();
    }

    #[test]
    #[should_panic]
    #[cfg(not(feature = "ssl"))]
    fn should_panic_if_verify_peer_query_param_requires_feature() {
        let opts = "mysql://usr:pw@localhost:3308/dbname?verify_peer=false";
        let _: Opts = opts.into();
    }

    #[test]
    #[should_panic]
    #[cfg(feature = "socket")]
    fn should_panic_on_invalid_prefer_socket_param_value() {
        let opts = "mysql://usr:pw@localhost:3308/dbname?prefer_socket=invalid";
        let _: Opts = opts.into();
    }

    #[test]
    #[should_panic]
    #[cfg(feature = "ssl")]
    fn should_panic_on_invalid_verify_peer_param_value() {
        let opts = "mysql://usr:pw@localhost:3308/dbname?verify_peer=invalid";
        let _: Opts = opts.into();
    }

    #[test]
    #[should_panic]
    #[cfg(all(not(feature = "ssl"), not(feature = "socket")))]
    fn should_panic_on_unk() {
        let opts = "mysql://localhost/dbname?prefer_socket=false";
        assert_eq!(Opts {
            user: Some("usr".to_string()),
            pass: Some("pw".to_string()),
            ip_or_hostname: Some("localhost".to_string()),
            tcp_port: 3308,
            db_name: Some("dbname".to_string()),
            ..Opts::default()
        }, opts.into());
    }
}
