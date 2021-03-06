use std::fmt;
use std::sync::{Arc, Mutex, Condvar};
use std::time::Duration as StdDuration;

use time::{Duration, SteadyTime};

use super::IsolationLevel;
use super::Transaction;
use super::super::error::{Error, DriverError};
use super::super::value::Params;
use super::{Conn, Opts, Stmt, QueryResult};
use super::super::error::Result as MyResult;

#[derive(Debug)]
struct InnerPool {
    opts: Opts,
    pool: Vec<Conn>,
    min: usize,
    max: usize,
    count: usize
}

impl InnerPool {
    fn new(min: usize, max: usize, opts: Opts) -> MyResult<InnerPool> {
        if min > max || max == 0 {
            return Err(Error::DriverError(DriverError::InvalidPoolConstraints));
        }
        let mut pool = InnerPool {
            opts: opts,
            pool: Vec::with_capacity(max),
            max: max,
            min: min,
            count: 0
        };
        for _ in 0..min {
            try!(pool.new_conn());
        }
        Ok(pool)
    }
    fn new_conn(&mut self) -> MyResult<()> {
        match Conn::new(self.opts.clone()) {
            Ok(conn) => {
                self.pool.push(conn);
                self.count += 1;
                Ok(())
            },
            Err(err) => Err(err)
        }
    }
}

/// `Pool` serves to provide you with a [`PooledConn`](struct.PooledConn.html)'s.
/// However you can prepare statements directly on `Pool` without
/// invoking [`Pool::get_conn`](struct.Pool.html#method.get_conn).
///
/// `Pool` will hold at least `min` connections and will create as many as `max`
/// connections.
///
/// Example of multithreaded `Pool` usage:
///
/// ```rust
/// use mysql::conn::pool;
/// use std::default::Default;
/// use mysql::conn::Opts;
/// # use mysql::conn::Row;
/// use std::thread;
///
/// fn get_opts() -> Opts {
///       // ...
/// #     let pwd: String = ::std::env::var("MYSQL_SERVER_PASS").unwrap_or("password".to_string());
/// #     let port: u16 = ::std::env::var("MYSQL_SERVER_PORT").ok()
/// #                                .map(|my_port| my_port.parse().ok().unwrap_or(3307))
/// #                                .unwrap_or(3307);
/// #     Opts {
/// #         user: Some("root".to_string()),
/// #         pass: Some(pwd),
/// #         ip_or_hostname: Some("127.0.0.1".to_string()),
/// #         tcp_port: port,
/// #         ..Default::default()
/// #     }
/// }
///
/// let opts = get_opts();
/// let pool = pool::Pool::new(opts).unwrap();
/// let mut threads = Vec::new();
/// for _ in 0..100 {
///     let pool = pool.clone();
///     threads.push(thread::spawn(move || {
///         let mut result = pool.prep_exec("SELECT 1", ()).unwrap();
///         assert_eq!(result.next().unwrap().unwrap(), Row::new(vec![1.into()]));
///     }));
/// }
/// for t in threads.into_iter() {
///     assert!(t.join().is_ok());
/// }
/// ```
///
/// For more info on how to work with mysql connection please look at
/// [`PooledConn`](struct.PooledConn.html) documentation.
#[derive(Clone)]
pub struct Pool(Arc<(Mutex<InnerPool>, Condvar)>);

impl Pool {
    /// Will return connection taken from a pool.
    ///
    /// Will verify and fix it via `Conn::ping` and `Conn::reset` if `call_ping` is `true`.
    /// Will try to get concrete connection if `id` is `Some(_)`.
    /// Will wait til timeout if `timeout_ms` is `Some(_)`
    fn _get_conn<T: AsRef<str>>(&self,
                                stmt: Option<T>,
                                timeout_ms: Option<u32>,
                                call_ping: bool) -> MyResult<PooledConn> {
        let times = if let Some(timeout_ms) = timeout_ms {
            Some ((
                SteadyTime::now(),
                Duration::milliseconds(timeout_ms as i64),
                StdDuration::from_millis(timeout_ms as u64),
            ))
        } else {
            None
        };

        let &(ref inner_pool, ref condvar) = &*self.0;
        let mut pool = match inner_pool.lock() {
            Ok(mutex) => mutex,
            _ => return Err(Error::DriverError(DriverError::PoisonedPoolMutex)),
        };

        let mut id = None;
        if let Some(query) = stmt {
            for (i, conn) in pool.pool.iter().enumerate() {
                if conn.has_stmt(query.as_ref()) {
                    id = Some(i);
                    break;
                }
            }
        }

        loop {
            if pool.pool.is_empty() {
                if pool.count < pool.max {
                    match pool.new_conn() {
                        Ok(()) => break,
                        Err(err) => return Err(err),
                    }
                } else {
                    pool = if let Some((start, timeout, std_timeout)) = times {
                        if SteadyTime::now() - start > timeout {
                            return Err(DriverError::Timeout.into());
                        }
                        match condvar.wait_timeout(pool, std_timeout) {
                            Ok((mutex, _)) => mutex,
                            _ => return Err(Error::DriverError(DriverError::PoisonedPoolMutex)),
                        }
                    } else {
                        match condvar.wait(pool) {
                            Ok(mutex) => mutex,
                            _ => return Err(Error::DriverError(DriverError::PoisonedPoolMutex)),
                        }
                    }
                }
            } else {
                break;
            }
        }

        let mut conn = if let Some(id) = id {
            if id < pool.pool.len() {
                pool.pool.remove(id)
            } else {
                pool.pool.pop().unwrap()
            }
        } else {
            pool.pool.pop().unwrap()
        };

        if call_ping {
            if !conn.ping() {
                try!(conn.reset());
            }
        }

        Ok(PooledConn {pool: self.clone(), conn: Some(conn)})
    }

    /// Creates new pool with `min = 10` and `max = 100`.
    pub fn new<T: Into<Opts>>(opts: T) -> MyResult<Pool> {
        Pool::new_manual(10, 100, opts)
    }

    /// Same as `new` but you can set `min` and `max`.
    pub fn new_manual<T: Into<Opts>>(min: usize, max: usize, opts: T) -> MyResult<Pool> {
        let pool = try!(InnerPool::new(min, max, opts.into()));
        Ok(Pool(Arc::new((Mutex::new(pool), Condvar::new()))))
    }

    /// Gives you a [`PooledConn`](struct.PooledConn.html).
    ///
    /// `Pool` will check that connection is alive via
    /// [`Conn::ping`](../struct.Conn.html#method.ping) and will
    /// call [`Conn::reset`](../struct.Conn.html#method.reset) if
    /// necessary.
    pub fn get_conn(&self) -> MyResult<PooledConn> {
        self._get_conn(None::<String>, None, true)
    }

    /// Will try to get connection for a duration of `timeout_ms` milliseconds.
    ///
    /// # Failure
    /// This function will return `Error::DriverError(DriverError::Timeout)` if timeout was
    /// reached while waiting for new connection to become available.
    pub fn try_get_conn(&self, timeout_ms: u32) -> MyResult<PooledConn> {
        self._get_conn(None::<String>, Some(timeout_ms), true)
    }

    fn get_conn_by_stmt<T: AsRef<str>>(&self, query: T) -> MyResult<PooledConn> {
        self._get_conn(Some(query), None, false)
    }

    /// Will prepare statement.
    ///
    /// It will try to find connection which has this statement cached.
    ///
    /// Will not check or fix connection health.
    pub fn prepare<'a, T: AsRef<str> + 'a>(&'a self, query: T) -> MyResult<Stmt<'a>> {
        let conn = try!(self.get_conn_by_stmt(query.as_ref()));
        conn.pooled_prepare(query)
    }

    /// Shortcut for `try!(pool.get_conn()).prep_exec(..)`.
    ///
    /// It will try to find connection which has this statement cached.
    ///
    /// Will not check or fix connection health.
    pub fn prep_exec<'a, A, T>(&'a self, query: A, params: T) -> MyResult<QueryResult<'a>>
    where A: AsRef<str>,
          T: Into<Params> {
        let conn = try!(self.get_conn_by_stmt(query.as_ref()));
        conn.pooled_prep_exec(query, params)
    }

    /// Shortcut for `try!(pool.get_conn()).start_transaction(..)`.
    pub fn start_transaction(&self,
                             consistent_snapshot: bool,
                             isolation_level: Option<IsolationLevel>,
                             readonly: Option<bool>) -> MyResult<Transaction> {
        (try!(self.get_conn())).pooled_start_transaction(consistent_snapshot,
                                                         isolation_level,
                                                         readonly)
    }
}

impl fmt::Debug for Pool {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let pool = (self.0).0.lock().unwrap();
        write!(f, "Pool {{ min: {}, max: {} }}", pool.min, pool.max)
    }
}

/// Pooled mysql connection which will return to the pool on `drop`.
///
/// You should prefer using `prepare` or `prep_exec` instead of `query` where possible, except
/// cases when statement has no params and when it has no return values or return values which
/// evaluates to `Value::Bytes`.
///
/// `query` is a part of mysql text protocol, so under the hood you will always receive
/// `Value::Bytes` as a result and `from_value` will need to parse it if you want, for example, `i64`
///
/// ```rust
/// # use mysql::conn::pool;
/// # use mysql::conn::Opts;
/// # use mysql::value::{from_value, Value};
/// # use std::thread::Thread;
/// # use std::default::Default;
/// # fn get_opts() -> Opts {
/// #     let pwd: String = ::std::env::var("MYSQL_SERVER_PASS").unwrap_or("password".to_string());
/// #     let port: u16 = ::std::env::var("MYSQL_SERVER_PORT").ok()
/// #                                .map(|my_port| my_port.parse().ok().unwrap_or(3307))
/// #                                .unwrap_or(3307);
/// #     Opts {
/// #         user: Some("root".to_string()),
/// #         pass: Some(pwd),
/// #         ip_or_hostname: Some("127.0.0.1".to_string()),
/// #         tcp_port: port,
/// #         ..Default::default()
/// #     }
/// # }
/// # let opts = get_opts();
/// # let pool = pool::Pool::new(opts).unwrap();
/// let mut conn = pool.get_conn().unwrap();
///
/// conn.query("SELECT 42").map(|mut result| {
///     let cell = result.next().unwrap().unwrap().take(0).unwrap();
///     assert_eq!(cell, Value::Bytes(b"42".to_vec()));
///     assert_eq!(from_value::<i64>(cell), 42i64);
/// });
/// conn.prep_exec("SELECT 42", ()).map(|mut result| {
///     let cell = result.next().unwrap().unwrap().take(0).unwrap();
///     assert_eq!(cell, Value::Int(42i64));
///     assert_eq!(from_value::<i64>(cell), 42i64);
/// });
/// ```
///
/// For more info on how to work with query results please look at
/// [`QueryResult`](../struct.QueryResult.html) documentation.
#[derive(Debug)]
pub struct PooledConn {
    pool: Pool,
    conn: Option<Conn>
}

impl Drop for PooledConn {
    fn drop(&mut self) {
        let mut pool = (self.pool.0).0.lock().unwrap();
        if pool.count > pool.min || self.conn.is_none() {
            pool.count -= 1;
        } else {
            pool.pool.push(self.conn.take().unwrap());
            (self.pool.0).1.notify_one();
        }
    }
}

impl PooledConn {
    /// Redirects to
    /// [`Conn#query`](../struct.Conn.html#method.query).
    pub fn query<'a, T: AsRef<str> + 'a>(&'a mut self, query: T) -> MyResult<QueryResult<'a>> {
        self.conn.as_mut().unwrap().query(query)
    }

    /// Redirects to
    /// [`Conn#prepare`](../struct.Conn.html#method.prepare).
    pub fn prepare<'a, T: AsRef<str> + 'a>(&'a mut self, query: T) -> MyResult<Stmt<'a>> {
        self.conn.as_mut().unwrap().prepare(query)
    }

    /// Redirects to
    /// [`Conn#prep_exec`](../struct.Conn.html#method.prep_exec).
    pub fn prep_exec<'a, A: AsRef<str> + 'a, T: Into<Params>>(&'a mut self, query: A, params: T) -> MyResult<QueryResult<'a>> {
        self.conn.as_mut().unwrap().prep_exec(query, params)
    }

    /// Redirects to
    /// [`Conn#start_transaction`](../struct.Conn.html#method.start_transaction)
    pub fn start_transaction<'a>(&'a mut self,
                                 consistent_snapshot: bool,
                                 isolation_level: Option<IsolationLevel>,
                                 readonly: Option<bool>) -> MyResult<Transaction<'a>> {
        self.conn.as_mut().unwrap().start_transaction(consistent_snapshot,
                                                      isolation_level,
                                                      readonly)
    }

    /// Gives mutable reference to the wrapped
    /// [`Conn`](../struct.Conn.html).
    pub fn as_mut<'a>(&'a mut self) -> &'a mut Conn {
        self.conn.as_mut().unwrap()
    }

    /// Gives reference to the wrapped
    /// [`Conn`](../struct.Conn.html).
    pub fn as_ref<'a>(&'a self) -> &'a Conn {
        self.conn.as_ref().unwrap()
    }

    /// Unwraps wrapped [`Conn`](../struct.Conn.html).
    pub fn unwrap(mut self) -> Conn {
        self.conn.take().unwrap()
    }

    fn pooled_prepare<'a, T: AsRef<str>>(mut self, query: T) -> MyResult<Stmt<'a>> {
        match self.as_mut()._prepare(query.as_ref()) {
            Ok(stmt) => Ok(Stmt::new_pooled(stmt, self)),
            Err(err) => Err(err)
        }
    }

    fn pooled_prep_exec<'a, A, T>(mut self, query: A, params: T) -> MyResult<QueryResult<'a>>
    where A: AsRef<str>,
          T: Into<Params>
    {
        let stmt = try!(self.as_mut()._prepare(query.as_ref()));
        let stmt = Stmt::new_pooled(stmt, self);
        stmt.prep_exec(params)
    }

    fn pooled_start_transaction<'a>(mut self,
                                    consistent_snapshot: bool,
                                    isolation_level: Option<IsolationLevel>,
                                    readonly: Option<bool>) -> MyResult<Transaction<'a>> {
        let _ = try!(self.as_mut()._start_transaction(consistent_snapshot,
                                                      isolation_level,
                                                      readonly));
        Ok(Transaction::new_pooled(self))
    }
}

#[cfg(test)]
#[allow(non_snake_case)]
mod test {
    use conn::Opts;
    use std::default::Default;

    pub static USER: &'static str = "root";
    pub static PASS: &'static str = "password";
    pub static ADDR: &'static str = "127.0.0.1";
    pub static PORT: u16          = 3307;

    #[cfg(feature = "openssl")]
    pub fn get_opts() -> Opts {
        let pwd: String = ::std::env::var("MYSQL_SERVER_PASS").unwrap_or(PASS.to_string());
        let port: u16 = ::std::env::var("MYSQL_SERVER_PORT").ok()
                                   .map(|my_port| my_port.parse().ok().unwrap_or(PORT))
                                   .unwrap_or(PORT);
        Opts {
            user: Some(USER.to_string()),
            pass: Some(pwd),
            ip_or_hostname: Some(ADDR.to_string()),
            tcp_port: port,
            ssl_opts: Some((::std::convert::From::from("tests/ca-cert.pem"), None)),
            ..Default::default()
        }
    }

    #[cfg(not(feature = "ssl"))]
    pub fn get_opts() -> Opts {
        let pwd: String = ::std::env::var("MYSQL_SERVER_PASS").unwrap_or(PASS.to_string());
        let port: u16 = ::std::env::var("MYSQL_SERVER_PORT").ok()
                                   .map(|my_port| my_port.parse().ok().unwrap_or(PORT))
                                   .unwrap_or(PORT);
        Opts {
            user: Some(USER.to_string()),
            pass: Some(pwd),
            ip_or_hostname: Some(ADDR.to_string()),
            tcp_port: port,
            ..Default::default()
        }
    }

    mod pool {
        use super::get_opts;
        use std::thread;
        use super::super::Pool;
        use super::super::super::super::value::from_value;
        use super::super::super::super::error::{Error, DriverError};
        #[test]
        fn should_execute_queryes_on_PooledConn() {
            let pool = Pool::new(get_opts()).unwrap();
            let mut threads = Vec::new();
            for _ in 0usize..10 {
                let pool = pool.clone();
                threads.push(thread::spawn(move || {
                    let conn = pool.get_conn();
                    assert!(conn.is_ok());
                    let mut conn = conn.unwrap();
                    assert!(conn.query("SELECT 1").is_ok());
                }));
            }
            for t in threads.into_iter() {
                assert!(t.join().is_ok());
            }
        }
        #[test]
        fn should_timeout_if_no_connections_available() {
            let pool = Pool::new_manual(0, 1, get_opts()).unwrap();
            let conn1 = pool.try_get_conn(357).unwrap();
            let conn2 = pool.try_get_conn(357);
            assert!(conn2.is_err());
            match conn2 {
                Err(Error::DriverError(DriverError::Timeout)) => assert!(true),
                _ => assert!(false),
            }
            drop(conn1);
            assert!(pool.try_get_conn(357).is_ok());
        }
        #[test]
        fn should_execute_statements_on_PooledConn() {
            let pool = Pool::new(get_opts()).unwrap();
            let mut threads = Vec::new();
            for _ in 0usize..10 {
                let pool = pool.clone();
                threads.push(thread::spawn(move || {
                    let mut conn = pool.get_conn().unwrap();
                    let mut stmt = conn.prepare("SELECT 1").unwrap();
                    assert!(stmt.execute(()).is_ok());
                }));
            }
            for t in threads.into_iter() {
                assert!(t.join().is_ok());
            }

            let pool = Pool::new(get_opts()).unwrap();
            let mut threads = Vec::new();
            for _ in 0usize..10 {
                let pool = pool.clone();
                threads.push(thread::spawn(move || {
                    let mut conn = pool.get_conn().unwrap();
                    conn.prep_exec("SELECT ?", (1,)).unwrap();
                }));
            }
            for t in threads.into_iter() {
                assert!(t.join().is_ok());
            }
        }
        #[test]
        fn should_execute_statements_on_Pool() {
            let pool = Pool::new(get_opts()).unwrap();
            let mut threads = Vec::new();
            for _ in 0usize..10 {
                let pool = pool.clone();
                threads.push(thread::spawn(move || {
                    let mut stmt = pool.prepare("SELECT 1").unwrap();
                    assert!(stmt.execute(()).is_ok());
                }));
            }
            for t in threads.into_iter() {
                assert!(t.join().is_ok());
            }

            let pool = Pool::new(get_opts()).unwrap();
            let mut threads = Vec::new();
            for _ in 0usize..10 {
                let pool = pool.clone();
                threads.push(thread::spawn(move || {
                    pool.prep_exec("SELECT ?", (1,)).unwrap();
                }));
            }
            for t in threads.into_iter() {
                assert!(t.join().is_ok());
            }
        }
        #[test]
        fn should_start_transaction_on_Pool() {
            let pool = Pool::new(get_opts()).unwrap();
            pool.prepare("CREATE TEMPORARY TABLE x.tbl(a INT)").ok().map(|mut stmt| {
                assert!(stmt.execute(()).is_ok());
            });
            assert!(pool.start_transaction(false, None, None).and_then(|mut t| {
                assert!(t.query("INSERT INTO x.tbl(a) VALUES(1)").is_ok());
                assert!(t.query("INSERT INTO x.tbl(a) VALUES(2)").is_ok());
                t.commit()
            }).is_ok());
            pool.prepare("SELECT COUNT(a) FROM x.tbl").ok().map(|mut stmt| {
                for x in stmt.execute(()).unwrap() {
                    let mut x = x.unwrap();
                    assert_eq!(from_value::<u8>(x.take(0).unwrap()), 2u8);
                }
            });
            assert!(pool.start_transaction(false, None, None).and_then(|mut t| {
                assert!(t.query("INSERT INTO x.tbl(a) VALUES(1)").is_ok());
                assert!(t.query("INSERT INTO x.tbl(a) VALUES(2)").is_ok());
                t.rollback()
            }).is_ok());
            pool.prepare("SELECT COUNT(a) FROM x.tbl").ok().map(|mut stmt| {
                for x in stmt.execute(()).unwrap() {
                    let mut x = x.unwrap();
                    assert_eq!(from_value::<u8>(x.take(0).unwrap()), 2u8);
                }
            });
            assert!(pool.start_transaction(false, None, None).and_then(|mut t| {
                assert!(t.query("INSERT INTO x.tbl(a) VALUES(1)").is_ok());
                assert!(t.query("INSERT INTO x.tbl(a) VALUES(2)").is_ok());
                Ok(())
            }).is_ok());
            pool.prepare("SELECT COUNT(a) FROM x.tbl").ok().map(|mut stmt| {
                for x in stmt.execute(()).unwrap() {
                    let mut x = x.unwrap();
                    assert_eq!(from_value::<u8>(x.take(0).unwrap()), 2u8);
                }
            });
        }
        #[test]
        fn should_start_transaction_on_PooledConn() {
            let pool = Pool::new(get_opts()).unwrap();
            let mut conn = pool.get_conn().unwrap();
            assert!(conn.query("CREATE TEMPORARY TABLE x.tbl(a INT)").is_ok());
            assert!(conn.start_transaction(false, None, None).and_then(|mut t| {
                assert!(t.query("INSERT INTO x.tbl(a) VALUES(1)").is_ok());
                assert!(t.query("INSERT INTO x.tbl(a) VALUES(2)").is_ok());
                t.commit()
            }).is_ok());
            for x in conn.query("SELECT COUNT(a) FROM x.tbl").unwrap() {
                let mut x = x.unwrap();
                assert_eq!(from_value::<u8>(x.take(0).unwrap()), 2u8);
            }
            assert!(conn.start_transaction(false, None, None).and_then(|mut t| {
                assert!(t.query("INSERT INTO x.tbl(a) VALUES(1)").is_ok());
                assert!(t.query("INSERT INTO x.tbl(a) VALUES(2)").is_ok());
                t.rollback()
            }).is_ok());
            for x in conn.query("SELECT COUNT(a) FROM x.tbl").unwrap() {
                let mut x = x.unwrap();
                assert_eq!(from_value::<u8>(x.take(0).unwrap()), 2u8);
            }
            assert!(conn.start_transaction(false, None, None).and_then(|mut t| {
                assert!(t.query("INSERT INTO x.tbl(a) VALUES(1)").is_ok());
                assert!(t.query("INSERT INTO x.tbl(a) VALUES(2)").is_ok());
                Ok(())
            }).is_ok());
            for x in conn.query("SELECT COUNT(a) FROM x.tbl").unwrap() {
                let mut x = x.unwrap();
                assert_eq!(from_value::<u8>(x.take(0).unwrap()), 2u8);
            }
        }
    }
}
