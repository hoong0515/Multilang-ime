//! 구간별 밑줄(표시 속성).
//!
//! | 상태 | 밑줄 | TSF 속성 |
//! | --- | --- | --- |
//! | 일반 조합 | 가는 실선 | TF_ATTR_INPUT |
//! | 모호한 구간 | 점선 | TF_ATTR_INPUT |
//! | 후보를 고르는 중 (M3) | 굵은 실선 | TF_ATTR_TARGET_CONVERTED |

use std::cell::Cell;

use windows::core::{implement, BSTR, GUID, Result};
use windows::Win32::Foundation::{E_INVALIDARG, S_FALSE};
use windows::Win32::UI::TextServices::*;

pub const ATTR_INPUT: GUID = GUID::from_u128(0x88892bcd_59f0_4872_b3f7_4bf0c8dcf30b);
pub const ATTR_AMBIGUOUS: GUID = GUID::from_u128(0x0e52490a_9127_40e0_9d9c_dd97478fcbc1);
pub const ATTR_FOCUSED: GUID = GUID::from_u128(0xa0d5d2d6_f321_41cf_9a8c_1a3484d5d55e);

pub const ALL: [GUID; 3] = [ATTR_INPUT, ATTR_AMBIGUOUS, ATTR_FOCUSED];

fn attribute(guid: &GUID) -> Option<(TF_DISPLAYATTRIBUTE, &'static str)> {
    let none = TF_DA_COLOR { r#type: TF_CT_NONE, ..Default::default() };
    let base = TF_DISPLAYATTRIBUTE { crText: none, crBk: none, crLine: none, ..Default::default() };
    Some(match *guid {
        ATTR_INPUT => (TF_DISPLAYATTRIBUTE { lsStyle: TF_LS_SOLID, fBoldLine: false.into(), bAttr: TF_ATTR_INPUT, ..base }, "Multilang input"),
        ATTR_AMBIGUOUS => (TF_DISPLAYATTRIBUTE { lsStyle: TF_LS_DOT, fBoldLine: false.into(), bAttr: TF_ATTR_INPUT, ..base }, "Multilang ambiguous"),
        ATTR_FOCUSED => (
            TF_DISPLAYATTRIBUTE { lsStyle: TF_LS_SOLID, fBoldLine: true.into(), bAttr: TF_ATTR_TARGET_CONVERTED, ..base },
            "Multilang focused",
        ),
        _ => return None,
    })
}

#[implement(ITfDisplayAttributeInfo)]
pub struct AttributeInfo {
    guid: GUID,
}

impl AttributeInfo {
    pub fn get(guid: &GUID) -> Result<ITfDisplayAttributeInfo> {
        if attribute(guid).is_none() {
            return Err(E_INVALIDARG.into());
        }
        Ok(AttributeInfo { guid: *guid }.into())
    }
}

impl ITfDisplayAttributeInfo_Impl for AttributeInfo_Impl {
    fn GetGUID(&self) -> Result<GUID> {
        Ok(self.guid)
    }

    fn GetDescription(&self) -> Result<BSTR> {
        Ok(BSTR::from(attribute(&self.guid).map(|a| a.1).unwrap_or("")))
    }

    fn GetAttributeInfo(&self, pda: *mut TF_DISPLAYATTRIBUTE) -> Result<()> {
        if pda.is_null() {
            return Err(E_INVALIDARG.into());
        }
        let (a, _) = attribute(&self.guid).ok_or(E_INVALIDARG)?;
        unsafe { *pda = a };
        Ok(())
    }

    fn SetAttributeInfo(&self, _pda: *const TF_DISPLAYATTRIBUTE) -> Result<()> {
        Ok(()) // 사용자 지정 색은 지원하지 않음
    }

    fn Reset(&self) -> Result<()> {
        Ok(())
    }
}

#[implement(IEnumTfDisplayAttributeInfo)]
pub struct EnumAttributes {
    index: Cell<usize>,
}

impl EnumAttributes {
    pub fn new(index: usize) -> IEnumTfDisplayAttributeInfo {
        EnumAttributes { index: Cell::new(index) }.into()
    }
}

impl IEnumTfDisplayAttributeInfo_Impl for EnumAttributes_Impl {
    fn Clone(&self) -> Result<IEnumTfDisplayAttributeInfo> {
        Ok(EnumAttributes::new(self.index.get()))
    }

    fn Next(&self, ulcount: u32, rginfo: *mut Option<ITfDisplayAttributeInfo>, pcfetched: *mut u32) -> Result<()> {
        if rginfo.is_null() {
            return Err(E_INVALIDARG.into());
        }
        let mut n = 0u32;
        while n < ulcount && self.index.get() < ALL.len() {
            let info = AttributeInfo::get(&ALL[self.index.get()])?;
            unsafe { rginfo.add(n as usize).write(Some(info)) };
            self.index.set(self.index.get() + 1);
            n += 1;
        }
        if !pcfetched.is_null() {
            unsafe { *pcfetched = n };
        }
        if n == ulcount { Ok(()) } else { Err(S_FALSE.into()) }
    }

    fn Reset(&self) -> Result<()> {
        self.index.set(0);
        Ok(())
    }

    fn Skip(&self, ulcount: u32) -> Result<()> {
        let next = self.index.get() + ulcount as usize;
        self.index.set(next.min(ALL.len()));
        if next <= ALL.len() { Ok(()) } else { Err(S_FALSE.into()) }
    }
}
