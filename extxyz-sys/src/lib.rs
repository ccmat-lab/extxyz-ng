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
        let entry = &*ptr;

        // Convert key
        let key = if !entry.key.is_null() {
            CStr::from_ptr(entry.key).to_string_lossy().into_owned()
        } else {
            panic!("Key cannot be null");
        };

        let ncols = entry.ncols as usize;
        let nrows = entry.nrows as usize;

        let value = match entry.data_t {
            data_type_data_i => {
                if nrows == 0 && ncols == 0 {
                    // scalar
                    let val = *(entry.data as *const i32);
                    Value::Int(val)
                } else if nrows == 0 {
                    // 1D array
                    let slice = slice::from_raw_parts(entry.data as *const i32, ncols);
                    Value::IntArray(slice.to_vec())
                } else {
                    // 2D array
                    let slice = slice::from_raw_parts(entry.data as *const i32, nrows * ncols);
                    let mut matrix = Vec::with_capacity(nrows);
                    for r in 0..nrows {
                        matrix.push(slice[r * ncols..(r + 1) * ncols].to_vec());
                    }
                    Value::MatrixInt(matrix)
                }
            }
            data_type_data_f => {
                if nrows == 0 && ncols == 0 {
                    let val = *(entry.data as *const f64);
                    Value::Float(val)
                } else if nrows == 0 {
                    let slice = slice::from_raw_parts(entry.data as *const f64, ncols);
                    Value::FloatArray(slice.to_vec())
                } else {
                    let slice = slice::from_raw_parts(entry.data as *const f64, nrows * ncols);
                    let mut matrix = Vec::with_capacity(nrows);
                    for r in 0..nrows {
                        matrix.push(slice[r * ncols..(r + 1) * ncols].to_vec());
                    }
                    Value::MatrixFloat(matrix)
                }
            }
            data_type_data_b => {
                if nrows == 0 && ncols == 0 {
                    let val = *(entry.data as *const i32) != 0; // C boolean as integer
                    Value::Bool(val)
                } else if nrows == 0 {
                    let slice = slice::from_raw_parts(entry.data as *const i32, ncols);
                    Value::BoolArray(slice.iter().map(|&v| v != 0).collect())
                } else {
                    let slice = slice::from_raw_parts(entry.data as *const i32, nrows * ncols);
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
                if nrows == 0 && ncols == 0 {
                    let cstr = CStr::from_ptr(entry.data as *const i8);
                    Value::Str(cstr.to_string_lossy().into_owned())
                } else if nrows == 0 {
                    // 1D array of strings
                    let slice = slice::from_raw_parts(entry.data as *const *const i8, ncols);
                    let vec: Vec<String> = slice
                        .iter()
                        .map(|&p| CStr::from_ptr(p).to_string_lossy().into_owned())
                        .collect();
                    Value::StrArray(vec)
                } else {
                    // 2D array of strings
                    let slice =
                        slice::from_raw_parts(entry.data as *const *const i8, nrows * ncols);
                    let mut matrix = Vec::with_capacity(nrows);
                    for r in 0..nrows {
                        let row = slice[r * ncols..(r + 1) * ncols]
                            .iter()
                            .map(|&p| CStr::from_ptr(p).to_string_lossy().into_owned())
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
        let inp = r#"8
Lattice="5.44 0.0 0.0 0.0 5.44 0.0 0.0 0.0 5.44" Properties=species:S:1:pos:R:3 Time=0.0
Si        0.00000000      0.00000000      0.00000000
Si        1.36000000      1.36000000      1.36000000
Si        2.72000000      2.72000000      0.00000000
Si        4.08000000      4.08000000      1.36000000
Si        2.72000000      0.00000000      2.72000000
Si        4.08000000      1.36000000      4.08000000
Si        0.00000000      2.72000000      2.72000000
Si        1.36000000      4.08000000      4.08000000
"#;
        let (natoms, info, arr, comment) = extxyz_read(inp).unwrap();

        // dbg!(natoms);
        dbg!(info);
        dbg!(arr);
        // dbg!(comment);
    }
}
