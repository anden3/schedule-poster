#[macro_export]
macro_rules! regex {
    ($re:literal $(,)?) => {{
        static RE: once_cell::sync::OnceCell<regex::Regex> = once_cell::sync::OnceCell::new();
        RE.get_or_init(|| regex::Regex::new($re).unwrap())
    }};
}

#[macro_export]
macro_rules! get_slash_commands {
    ($v:ident, $($g:ident),*) => {
        let mut $v = Vec::new();

        $(
            paste::paste! {
                for cmd in commands::[<$g:upper _SLASH_GROUP_OPTIONS>].commands {
                    $v.push((cmd, commands::[<$g:upper _SLASH_GROUP>].options));
                }
            }
        )*
    }
}

#[macro_export]
macro_rules! client_data_types {
    ($($t:ty),*) => {
        $(
            impl TypeMapKey for $t {
                type Value = Self;
            }
        )*
    }
}

#[macro_export]
macro_rules! wrap_type_aliases {
    ($($n:ident|$t:ty),*) => {
        $(
            pub struct $n($t);

            impl Deref for $n {
                type Target = $t;

                fn deref(&self) -> &Self::Target {
                    &self.0
                }
            }
        )*
    }
}

#[macro_export]
macro_rules! define_command_group {
    ($g:ident, [$($c:ident),*]) => {
        $(
            mod $c;
        )*

        $(
            paste::paste! { use $c::[<$c:upper _COMMAND>]; }
        )*

        #[group]
        #[commands(
            $(
                $c,
            )*
        )]
        struct $g;
    }
}

#[macro_export]
macro_rules! define_slash_command_group {
    ($g:ident, [$($c:ident),*]) => {
        $(
            pub mod $c;
        )*

        $(
            paste::paste! { use $c::[<$c:upper _SLASH_COMMAND>]; }
        )*

        #[slash_group]
        #[commands(
            $(
                $c,
            )*
        )]
        struct $g;
    }
}
