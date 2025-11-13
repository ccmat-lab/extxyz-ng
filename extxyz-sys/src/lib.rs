#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(unnecessary_transmutes)]
#![allow(clippy::if_not_else)]

use std::{
    collections::HashMap,
    ffi::{CStr, CString},
    io, slice,
};

use libc::fmemopen;

include!("bindings.rs");

#[derive(Debug)]
pub enum Value {
    Int(i32),
    Float(f64),
    Bool(bool),
    Str(String),
    IntArray(Vec<i32>),
    FloatArray(Vec<f64>),
    BoolArray(Vec<bool>),
    StrArray(Vec<String>),
    MatrixInt(Vec<Vec<i32>>),
    MatrixFloat(Vec<Vec<f64>>),
    MatrixBool(Vec<Vec<bool>>),
    MatrixStr(Vec<Vec<String>>),
    Dict(HashMap<String, Value>),
    Unsupported,
}

#[allow(clippy::too_many_lines)]
#[allow(clippy::cast_sign_loss)]
unsafe fn c_to_rust_dict(mut ptr: *mut dict_entry_struct) -> HashMap<String, Value> {
    let mut map = HashMap::new();

    while !ptr.is_null() {
        let entry = unsafe { &*ptr };

        // Convert key
        let key = if !entry.key.is_null() {
            unsafe { CStr::from_ptr(entry.key).to_string_lossy().into_owned() }
        } else {
            panic!("Key cannot be null");
        };

        // Ensure at least 1 row/col for compatibility with C loops
        let nrows = if entry.nrows < 1 {
            1
        } else {
            entry.nrows as usize
        };
        let ncols = if entry.ncols < 1 {
            1
        } else {
            entry.ncols as usize
        };

        let value = match entry.data_t {
            data_type_data_i => {
                let slice =
                    unsafe { slice::from_raw_parts(entry.data as *const i32, nrows * ncols) };
                if nrows == 1 && ncols == 1 {
                    Value::Int(slice[0])
                } else if nrows == 1 {
                    Value::IntArray(slice.to_vec())
                } else {
                    let mut matrix = Vec::with_capacity(nrows);
                    for r in 0..nrows {
                        matrix.push(slice[r * ncols..(r + 1) * ncols].to_vec());
                    }
                    Value::MatrixInt(matrix)
                }
            }

            data_type_data_f => {
                let slice =
                    unsafe { slice::from_raw_parts(entry.data as *const f64, nrows * ncols) };
                if nrows == 1 && ncols == 1 {
                    Value::Float(slice[0])
                } else if nrows == 1 {
                    Value::FloatArray(slice.to_vec())
                } else {
                    let mut matrix = Vec::with_capacity(nrows);
                    for r in 0..nrows {
                        matrix.push(slice[r * ncols..(r + 1) * ncols].to_vec());
                    }
                    Value::MatrixFloat(matrix)
                }
            }

            data_type_data_b => {
                let slice =
                    unsafe { slice::from_raw_parts(entry.data as *const i32, nrows * ncols) };
                if nrows == 1 && ncols == 1 {
                    Value::Bool(slice[0] != 0)
                } else if nrows == 1 {
                    Value::BoolArray(slice.iter().map(|&v| v != 0).collect())
                } else {
                    let mut matrix = Vec::with_capacity(nrows);
                    for r in 0..nrows {
                        matrix.push(
                            slice[r * ncols..(r + 1) * ncols]
                                .iter()
                                .map(|&v| v != 0)
                                .collect(),
                        );
                    }
                    Value::MatrixBool(matrix)
                }
            }

            data_type_data_s => {
                let slice =
                    unsafe { slice::from_raw_parts(entry.data as *const *const i8, nrows * ncols) };
                if nrows == 1 && ncols == 1 {
                    let s = unsafe { CStr::from_ptr(slice[0]).to_string_lossy().into_owned() };
                    Value::Str(s)
                } else if nrows == 1 {
                    let vec: Vec<String> = slice
                        .iter()
                        .map(|&p| unsafe { CStr::from_ptr(p).to_string_lossy().into_owned() })
                        .collect();
                    Value::StrArray(vec)
                } else {
                    let mut matrix = Vec::with_capacity(nrows);
                    for r in 0..nrows {
                        let row = slice[r * ncols..(r + 1) * ncols]
                            .iter()
                            .map(|&p| unsafe { CStr::from_ptr(p).to_string_lossy().into_owned() })
                            .collect();
                        matrix.push(row);
                    }
                    Value::MatrixStr(matrix)
                }
            }

            _ => Value::Unsupported,
        };

        map.insert(key, value);

        ptr = entry.next;
    }

    map
}

// Safe hardler for `DictEntry`
#[derive(Debug)]
pub struct DictHandler {
    data: HashMap<String, Value>,
}

impl DictHandler {
    pub unsafe fn new(ptr: *mut dict_entry_struct) -> Self {
        let data = unsafe { c_to_rust_dict(ptr) };
        DictHandler { data }
    }
}

/// Safe wrapper of unsafe c api for `extxyz_read_ll` function
/// It returns, (natoms, info, arrays, comments) as a fallible result
///
/// # Errors
///
/// error when input contains null byte
/// error when unable to read input in unsafe block
/// error when unsafe ``extxyz_read_ll`` call failed
pub fn extxyz_read(input: &str) -> Result<(i32, DictHandler, DictHandler, String), io::Error> {
    let kv_grammar = unsafe { compile_extxyz_kv_grammar() };

    // Prepare output variables
    let mut nat: i32 = 0;

    // allocate buffer for comment
    let mut comment_buf = vec![0u8; 2048];
    let comment_ptr = comment_buf.as_mut_ptr().cast::<i8>();

    // allocate pointer for info ptr and arrays ptr
    // NOTE: the info and arrays are ptr and they are allocated within the `extxyz_read_ll`
    // unsafe call through `tree_to_dict`
    let mut info: *mut DictEntry = std::ptr::null_mut();
    let mut arrays: *mut DictEntry = std::ptr::null_mut();

    let mut error_message = vec![0u8; 1024];
    let error_ptr = error_message.as_mut_ptr().cast::<i8>();

    let ret = unsafe {
        let mut bytes = input.as_bytes().to_vec();
        let fp = fmemopen(
            bytes.as_mut_ptr().cast::<libc::c_void>(),
            bytes.len(),
            CString::new("r").unwrap().as_ptr(),
        );
        if fp.is_null() {
            return Err(io::Error::other("Failed to open file"));
        }

        extxyz_read_ll(
            kv_grammar,
            fp,
            &raw mut nat,
            &raw mut info,
            &raw mut arrays,
            comment_ptr,
            error_ptr,
        )
    };

    let err_msg = unsafe {
        let err_cstr = CStr::from_ptr(error_ptr.cast_const());
        err_cstr.to_string_lossy()
    };

    // NOTE: extxyz use 1 for success, which is .... those fucking scientist.
    if ret != 1 {
        return Err(io::Error::other(format!(
            "extxyz_read_ll failed, errno: {ret}, msg: {err_msg}",
        )));
    }

    // convert comment buffer to Rust String
    let comment = unsafe {
        std::ffi::CStr::from_ptr(comment_ptr)
            .to_string_lossy()
            .into_owned()
    };

    unsafe {
        print_dict(info);
    };

    // own the dict and it will be dropped after use.
    let (info_val, arrays_val) = unsafe { (DictHandler::new(info), DictHandler::new(arrays)) };

    unsafe {
        free_dict(info);
        free_dict(arrays);
    }

    Ok((nat, info_val, arrays_val, comment))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn extxyz_read_default() {
        let inp = r#"4
key1=a key2=a/b key3=a@b key4="a@b"
Mg        -4.25650        3.79180       -2.54123
C         -1.15405        2.86652       -1.26699
C         -5.53758        3.70936        0.63504
C         -7.28250        4.71303       -3.82016
"#;
        let (natoms, info, arr, comment) = extxyz_read(inp).unwrap();

        // dbg!(natoms);
        dbg!(info);
        // dbg!(arr);
        // dbg!(comment);
    }
}
