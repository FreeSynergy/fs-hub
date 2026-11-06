//! The typed object envelope on the wire.
//!
//! Pattern: Adapter — `fs_store::Envelope` <-> `common.capnp` `Envelope` (routing summary incl. `describes`, `startTime`, `endTime`).

use fs_store::{Envelope, ObjectId};

use super::ids::{from_ms, ms, read_uuid, write_uuid};
use super::lists::{set_text_list, text_list};
use crate::common_capnp as common;
use crate::{Error, Result};

/// Fill the typed routing summary of an object.
pub fn write_envelope(mut b: common::envelope::Builder<'_>, e: &Envelope) {
    write_uuid(b.reborrow().init_id(), Some(e.id.as_uuid()));
    b.set_iri(e.iri.as_deref().unwrap_or(""));
    b.set_type(&e.object_type);
    b.set_name(&e.name);
    b.set_attributed_to(e.attributed_to.as_deref().unwrap_or(""));
    set_text_list(b.reborrow().init_to(e.to.len() as u32), &e.to);
    set_text_list(b.reborrow().init_cc(e.cc.len() as u32), &e.cc);
    set_text_list(b.reborrow().init_bto(e.bto.len() as u32), &e.bto);
    set_text_list(b.reborrow().init_bcc(e.bcc.len() as u32), &e.bcc);
    set_text_list(
        b.reborrow().init_audience(e.audience.len() as u32),
        &e.audience,
    );
    b.set_context(e.context.as_deref().unwrap_or(""));
    b.set_in_reply_to(e.in_reply_to.as_deref().unwrap_or(""));
    b.reborrow()
        .init_published()
        .set_unix_millis(ms(e.published));
    b.reborrow().init_updated().set_unix_millis(ms(e.updated));
    set_text_list(
        b.reborrow().init_sensitive(e.sensitive.len() as u32),
        &e.sensitive,
    );
    b.set_describes(e.describes.as_deref().unwrap_or(""));
    if let Some(t) = e.start_time {
        b.reborrow().init_start_time().set_unix_millis(ms(t));
    }
    if let Some(t) = e.end_time {
        b.reborrow().init_end_time().set_unix_millis(ms(t));
    }
}

fn opt_text(r: capnp::text::Reader<'_>) -> Result<Option<String>> {
    let s = r.to_string()?;
    Ok((!s.is_empty()).then_some(s))
}

/// Read the typed envelope back (empty text = none, absent timestamp = none).
pub fn read_envelope(r: common::envelope::Reader<'_>) -> Result<Envelope> {
    let id = read_uuid(r.get_id()?).ok_or_else(|| Error::Invalid("envelope without id".into()))?;
    let mut e = Envelope::new(r.get_type()?.to_string()?, r.get_name()?.to_string()?);
    e.id = ObjectId::from_uuid(id)?;
    e.iri = opt_text(r.get_iri()?)?;
    e.attributed_to = opt_text(r.get_attributed_to()?)?;
    e.to = text_list(r.get_to()?)?;
    e.cc = text_list(r.get_cc()?)?;
    e.bto = text_list(r.get_bto()?)?;
    e.bcc = text_list(r.get_bcc()?)?;
    e.audience = text_list(r.get_audience()?)?;
    e.context = opt_text(r.get_context()?)?;
    e.in_reply_to = opt_text(r.get_in_reply_to()?)?;
    e.published = from_ms(r.get_published()?.get_unix_millis());
    e.updated = from_ms(r.get_updated()?.get_unix_millis());
    e.sensitive = text_list(r.get_sensitive()?)?;
    e.describes = opt_text(r.get_describes()?)?;
    e.start_time = if r.has_start_time() {
        Some(from_ms(r.get_start_time()?.get_unix_millis()))
    } else {
        None
    };
    e.end_time = if r.has_end_time() {
        Some(from_ms(r.get_end_time()?.get_unix_millis()))
    } else {
        None
    };
    Ok(e)
}
