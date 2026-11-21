//! ABI C de la façade, chargée par Python via `ctypes`.
//!
//! Toutes les fonctions prennent et renvoient des chaînes UTF-8 terminées par
//! un zéro. Les chaînes renvoyées appartiennent à l'appelant, qui doit les
//! libérer avec [`hexbandit_free_string`]. Aucune panique ne traverse la
//! frontière : elles sont interceptées et converties en erreur `internal`.
//! `ctypes` libère le GIL pendant l'appel ; l'annulation passe par
//! [`hexbandit_cancel`], appelée depuis un autre thread Python.

use std::{
    ffi::{CStr, CString, c_char},
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
    sync::LazyLock,
};

use mcts::CancelToken;

use crate::{error::BindingError, facade, json::Json, log};

static CANCEL: LazyLock<CancelToken> = LazyLock::new(CancelToken::new);

fn to_c_string(text: String) -> *mut c_char {
    match CString::new(text) {
        Ok(c) => c.into_raw(),
        // Impossible en pratique : le JSON produit échappe les octets nuls.
        Err(_) => ptr::null_mut(),
    }
}

/// Lit l'entrée, exécute `body` en interceptant les paniques et renvoie
/// l'enveloppe JSON.
///
/// # Safety
///
/// `input` doit être nul ou pointer vers une chaîne C valide pendant l'appel.
unsafe fn call(
    input: *const c_char,
    body: impl FnOnce(&str) -> Result<Json, BindingError>,
) -> *mut c_char {
    let text = if input.is_null() {
        Err(BindingError::config("pointeur nul"))
    } else {
        // SAFETY : l'appelant garantit une chaîne C valide pendant l'appel.
        let cstr = unsafe { CStr::from_ptr(input) };
        cstr.to_str()
            .map_err(|_| BindingError::config("entrée non UTF-8"))
    };
    let result = match text {
        Err(e) => Err(e),
        Ok(text) => match catch_unwind(AssertUnwindSafe(|| body(text))) {
            Ok(result) => result,
            Err(payload) => {
                let message = payload
                    .downcast_ref::<&str>()
                    .map(|s| (*s).to_owned())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "panique".to_owned());
                log::error(&format!("panique interceptée : {message}"));
                Err(BindingError::internal(message))
            }
        },
    };
    to_c_string(facade::envelope(result))
}

/// Version du moteur et des formats (enveloppe JSON).
#[unsafe(no_mangle)]
pub extern "C" fn hexbandit_version() -> *mut c_char {
    to_c_string(facade::envelope(Ok(facade::version())))
}

/// Joue une partie décrite par `config` (JSON) et renvoie son enregistrement.
///
/// # Safety
///
/// `config` doit être une chaîne C valide (ou nulle).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hexbandit_play_game(config: *const c_char) -> *mut c_char {
    CANCEL.reset();
    // SAFETY : contrat de la fonction.
    unsafe { call(config, |text| facade::play_game(text, &CANCEL)) }
}

/// Joue un lot de parties sur `threads` threads.
///
/// # Safety
///
/// `request` doit être une chaîne C valide (ou nulle).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hexbandit_play_batch(request: *const c_char, threads: u32) -> *mut c_char {
    CANCEL.reset();
    // SAFETY : contrat de la fonction.
    unsafe {
        call(request, |text| {
            facade::play_batch(text, threads as usize, &CANCEL)
        })
    }
}

/// Recherche sur une position.
///
/// # Safety
///
/// `request` doit être une chaîne C valide (ou nulle).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hexbandit_search(request: *const c_char) -> *mut c_char {
    CANCEL.reset();
    // SAFETY : contrat de la fonction.
    unsafe { call(request, |text| facade::search(text, &CANCEL)) }
}

/// Demande l'interruption du calcul en cours (Ctrl-C côté Python).
#[unsafe(no_mangle)]
pub extern "C" fn hexbandit_cancel() {
    CANCEL.cancel();
}

/// Libère une chaîne renvoyée par la bibliothèque.
///
/// # Safety
///
/// `text` doit avoir été renvoyée par une fonction `hexbandit_*` et ne pas avoir
/// déjà été libérée.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hexbandit_free_string(text: *mut c_char) {
    if !text.is_null() {
        // SAFETY : la chaîne provient de CString::into_raw (contrat).
        drop(unsafe { CString::from_raw(text) });
    }
}
