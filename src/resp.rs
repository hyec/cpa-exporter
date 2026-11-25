use std::{
    io::{self, BufRead, Write},
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Debug, PartialEq)]
pub enum Value {
    Bytes(Vec<u8>),
    Number(i64),
    Array(Vec<Value>),
    Null,
}
impl Value {
    pub fn bytes(&self) -> Option<&[u8]> {
        if let Self::Bytes(b) = self {
            Some(b)
        } else {
            None
        }
    }
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid RESP frame")
}
fn exact(reader: &mut impl BufRead, mut out: &mut [u8], stop: &AtomicBool) -> io::Result<()> {
    while !out.is_empty() {
        if stop.load(Ordering::Relaxed) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        match reader.read(out) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => out = &mut out[n..],
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::TimedOut
                        | io::ErrorKind::WouldBlock
                        | io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
fn line(reader: &mut impl BufRead, stop: &AtomicBool) -> io::Result<Vec<u8>> {
    let mut result = Vec::new();
    loop {
        if result.len() > 8192 {
            return Err(invalid());
        }
        let mut b = [0];
        exact(reader, &mut b, stop)?;
        result.push(b[0]);
        if b[0] == b'\n' {
            if result.len() < 2 || result[result.len() - 2] != b'\r' {
                return Err(invalid());
            }
            result.truncate(result.len() - 2);
            return Ok(result);
        }
    }
}
pub fn read(reader: &mut impl BufRead, stop: &AtomicBool) -> io::Result<Value> {
    read_depth(reader, stop, 0)
}
fn read_depth(reader: &mut impl BufRead, stop: &AtomicBool, depth: usize) -> io::Result<Value> {
    if depth > 8 || stop.load(Ordering::Relaxed) {
        return Err(invalid());
    }
    let mut prefix = [0];
    // An idle timeout is returned only before a frame starts. Partial frames
    // keep their parse state across timeouts instead of desynchronizing RESP.
    reader.read_exact(&mut prefix)?;
    let header = line(reader, stop)?;
    match prefix[0] {
        b'+' => Ok(Value::Bytes(header)),
        b'-' => Err(io::Error::other("Redis server error")),
        b':' => Ok(Value::Number(
            std::str::from_utf8(&header)
                .map_err(|_| invalid())?
                .parse()
                .map_err(|_| invalid())?,
        )),
        b'$' | b'*' => {
            let size: i64 = std::str::from_utf8(&header)
                .map_err(|_| invalid())?
                .parse()
                .map_err(|_| invalid())?;
            if size == -1 {
                return Ok(Value::Null);
            }
            if size < 0 {
                return Err(invalid());
            }
            if prefix[0] == b'$' {
                if size > 16 * 1024 * 1024 {
                    return Err(invalid());
                }
                let mut bytes = vec![0; size as usize + 2];
                exact(reader, &mut bytes, stop)?;
                if bytes[size as usize..] != *b"\r\n" {
                    return Err(invalid());
                }
                bytes.truncate(size as usize);
                Ok(Value::Bytes(bytes))
            } else {
                if size > 1024 {
                    return Err(invalid());
                }
                let mut values = Vec::with_capacity(size as usize);
                for _ in 0..size {
                    // Wait for a nested frame before parsing it, keeping state
                    // even when the server pauses between array elements.
                    loop {
                        match reader.fill_buf() {
                            Err(e)
                                if matches!(
                                    e.kind(),
                                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                                ) =>
                            {
                                if stop.load(Ordering::Relaxed) {
                                    return Err(io::ErrorKind::Interrupted.into());
                                }
                            }
                            Err(e) => return Err(e),
                            Ok(_) => break,
                        }
                    }
                    values.push(read_depth(reader, stop, depth + 1)?);
                }
                Ok(Value::Array(values))
            }
        }
        _ => Err(invalid()),
    }
}
pub fn write_command(w: &mut impl Write, args: &[&str]) -> io::Result<()> {
    write!(w, "*{}\r\n", args.len())?;
    for arg in args {
        write!(w, "${}\r\n{}\r\n", arg.len(), arg)?;
    }
    w.flush()
}
pub fn usage_message(value: &Value) -> Option<&[u8]> {
    if let Value::Array(items) = value {
        if items.len() == 3
            && items[0].bytes()?.eq_ignore_ascii_case(b"message")
            && items[1].bytes()?.eq_ignore_ascii_case(b"usage")
        {
            return items[2].bytes();
        }
    }
    None
}
pub fn is_subscription(value: &Value) -> bool {
    matches!(value,Value::Array(items) if items.len()==3 && items[0].bytes()==Some(b"subscribe") && items[1].bytes()==Some(b"usage") && matches!(items[2],Value::Number(1)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pubsub_and_commands() {
        let mut output = vec![];
        write_command(&mut output, &["AUTH", "secret"]).unwrap();
        assert_eq!(output, b"*2\r\n$4\r\nAUTH\r\n$6\r\nsecret\r\n");
        let value = read(
            &mut &b"*3\r\n$7\r\nmessage\r\n$5\r\nusage\r\n$2\r\n{}\r\n"[..],
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(usage_message(&value), Some(&b"{}"[..]));
        for bad in [
            &b"$999999999\r\n"[..],
            b"*99999\r\n",
            b"$2\r\n{}xx",
            b"+bad\n",
        ] {
            assert!(read(&mut &*bad, &AtomicBool::new(false)).is_err());
        }
    }
}
