pub mod minecraft;

#[cfg(test)]
mod truncate_to_zero {
    use crate::minecraft::serialization::truncate_to_zero;

    #[test]
    fn with_zero() {
        let actual = truncate_to_zero("hello\0world");
        assert_eq!(actual, "hello");
    }

    #[test]
    fn without_zero() {
        let actual = truncate_to_zero("no-zero");
        assert_eq!(actual, "no-zero");
    }

    #[test]
    fn regular_domain() {
        let actual = truncate_to_zero("folleach.net");
        assert_eq!(actual, "folleach.net");
    }

    #[test]
    fn trailing_zeros() {
        let actual = truncate_to_zero("sinya.ru\0\0\0\0");
        assert_eq!(actual, "sinya.ru");
    }

    #[test]
    fn emptry_string() {
        let actual = truncate_to_zero("");
        assert_eq!(actual, "");
    }
}
