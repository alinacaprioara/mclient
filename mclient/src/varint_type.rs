use ::std::io;

const SEGMENT_BIT: i32 = 0x7F;
const CONTINUE_BIT: i32 = 0x80;

pub fn varint_read(bytes: &mut Vec<u8>) -> Result<i32, io::Error> {
    let mut value = 0;
    let mut position = 0;
    let mut current_byte: u8;

    loop {
        current_byte = bytes.remove(0);
        value |= (current_byte as i32 & SEGMENT_BIT) << position;

        if (current_byte as i32 & CONTINUE_BIT) == 0 {
            break;
        }

        position += 7;

        if position >= 32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "VarInt too big",
            ));
        }
    }
    Ok(value)
}

pub fn varint_write(mut value: i32) -> Vec<u8> {
    let mut res: Vec<u8> = vec![];

    loop {
        if (value & !SEGMENT_BIT) == 0 {
            res.push(value as u8);
            break;
        }

        res.push(((value & SEGMENT_BIT) | CONTINUE_BIT) as u8);

        value >>= 7;
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read() {
        assert_eq!(0, varint_read(vec![0x00].as_mut()).unwrap());
        assert_eq!(1, varint_read(vec![0x01].as_mut()).unwrap());
        assert_eq!(2, varint_read(vec![0x02].as_mut()).unwrap());
        assert_eq!(127, varint_read(vec![0x7f].as_mut()).unwrap());
        assert_eq!(128, varint_read(vec![0x80, 0x01].as_mut()).unwrap());
        assert_eq!(255, varint_read(vec![0xff, 0x01].as_mut()).unwrap());
        assert_eq!(25565, varint_read(vec![0xdd, 0xc7, 0x01].as_mut()).unwrap());
        assert_eq!(
            2097151,
            varint_read(vec![0xff, 0xff, 0x7f].as_mut()).unwrap()
        );
        assert_eq!(
            2147483647,
            varint_read(vec![0xff, 0xff, 0xff, 0xff, 0x07].as_mut()).unwrap()
        );
        assert_eq!(
            -1,
            varint_read(vec![0xff, 0xff, 0xff, 0xff, 0x0f].as_mut()).unwrap()
        );
        assert_eq!(
            -2147483648,
            varint_read(vec![0x80, 0x80, 0x80, 0x80, 0x08].as_mut()).unwrap()
        );
    }

    #[test]
    fn test_write() {
        assert_eq!(vec![0x00], varint_write(0));
        assert_eq!(vec![0x01], varint_write(1));
        assert_eq!(vec![0x02], varint_write(2));
        assert_eq!(vec![0x7f], varint_write(127));
        assert_eq!(vec![0x80, 0x01], varint_write(128));
        assert_eq!(vec![0xff, 0x01], varint_write(255));
        assert_eq!(vec![0xdd, 0xc7, 0x01], varint_write(25565));
        assert_eq!(vec![0xff, 0xff, 0x7f], varint_write(2097151));
        assert_eq!(vec![0xff, 0xff, 0xff, 0xff, 0x07], varint_write(2147483647));
        assert_eq!(vec![0xff, 0xff, 0xff, 0xff, 0x0f], varint_write(-1));
        assert_eq!(
            vec![0x80, 0x80, 0x80, 0x80, 0x08],
            varint_write(-2147483648)
        );
    }
}
