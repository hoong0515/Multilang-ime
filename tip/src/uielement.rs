//! 후보 목록 UI 요소 (ITfCandidateListUIElement).
//!
//! 후보창을 띄울 때 TSF의 UI 요소 관리자에 알린다 (BeginUIElement). 앱이 직접 그리겠다고 하면(bShow=false)
//! 우리 창은 숨기고, 앱은 이 객체에서 후보 목록을 읽어 그린다 (게임 등 UI-less 앱, 셸 검색창 시험).

use std::cell::RefCell;
use std::rc::Rc;

use windows::core::{implement, BOOL, BSTR, GUID, Result};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG};
use windows::Win32::UI::TextServices::*;

/// 이 UI 요소 종류의 GUID.
pub const GUID_CANDIDATE_UI: GUID = GUID::from_u128(0x7c1f2a64_5b3e_4d21_9a0f_3e8b6c9d1a52);

#[derive(Default)]
pub struct Data {
    pub items: Vec<String>,
    pub selected: u32,
    pub flags: u32,
    pub shown: bool,
    pub docmgr: Option<ITfDocumentMgr>,
}

#[implement(ITfCandidateListUIElement, ITfUIElement)]
pub struct CandidateUi {
    pub data: Rc<RefCell<Data>>,
}

impl ITfUIElement_Impl for CandidateUi_Impl {
    fn GetDescription(&self) -> Result<BSTR> {
        Ok(BSTR::from("Multilang IME candidate list"))
    }
    fn GetGUID(&self) -> Result<GUID> {
        Ok(GUID_CANDIDATE_UI)
    }
    fn Show(&self, bshow: BOOL) -> Result<()> {
        if let Ok(mut d) = self.data.try_borrow_mut() {
            d.shown = bshow.as_bool();
        }
        crate::debug_note(&format!("uielement Show({})", bshow.as_bool()));
        Ok(())
    }
    fn IsShown(&self) -> Result<BOOL> {
        Ok(self.data.try_borrow().map(|d| d.shown).unwrap_or(false).into())
    }
}

impl ITfCandidateListUIElement_Impl for CandidateUi_Impl {
    fn GetUpdatedFlags(&self) -> Result<u32> {
        let mut d = self.data.try_borrow_mut().map_err(|_| E_FAIL)?;
        let f = d.flags;
        d.flags = 0;
        Ok(f)
    }
    fn GetDocumentMgr(&self) -> Result<ITfDocumentMgr> {
        self.data.try_borrow().ok().and_then(|d| d.docmgr.clone()).ok_or_else(|| E_FAIL.into())
    }
    fn GetCount(&self) -> Result<u32> {
        Ok(self.data.try_borrow().map(|d| d.items.len() as u32).unwrap_or(0))
    }
    fn GetSelection(&self) -> Result<u32> {
        Ok(self.data.try_borrow().map(|d| d.selected).unwrap_or(0))
    }
    fn GetString(&self, uindex: u32) -> Result<BSTR> {
        let d = self.data.try_borrow().map_err(|_| E_FAIL)?;
        d.items.get(uindex as usize).map(|s| BSTR::from(s.as_str())).ok_or_else(|| E_INVALIDARG.into())
    }
    fn GetPageIndex(&self, pindex: *mut u32, usize: u32, pupagecnt: *mut u32) -> Result<()> {
        // 한 쪽짜리 목록
        unsafe {
            if !pupagecnt.is_null() {
                *pupagecnt = 1;
            }
            if !pindex.is_null() && usize >= 1 {
                *pindex = 0;
            }
        }
        Ok(())
    }
    fn SetPageIndex(&self, _pindex: *const u32, _upagecnt: u32) -> Result<()> {
        Ok(())
    }
    fn GetCurrentPage(&self) -> Result<u32> {
        Ok(0)
    }
}
