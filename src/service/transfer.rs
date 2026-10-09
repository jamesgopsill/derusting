pub struct FileTransfer<V>
where
    V: super::Vfs,
{
    fil: V,
    id: u32,
    guid: uuid::Uuid,
}

impl<V> FileTransfer<V>
where
    V: super::Vfs,
{
    pub async fn new(gcode: &super::message::SharedGcode<'_>) -> Result<Self, V::Error> {
        // TODO: include gcode id check but needs to map to V::Error
        let partial_path = heapless::format!(64; "/usb/{}.partial", gcode.guid).unwrap();
        let mut fil = V::open(&partial_path, super::VfsFlag::Write).await?;
        fil.write_all(gcode.data)?;
        Ok(Self {
            fil,
            id: 0,
            guid: gcode.guid,
        })
    }

    pub async fn digest(
        mut self,
        gcode: &super::message::SharedGcode<'_>,
    ) -> Result<Option<Self>, V::Error> {
        // NOTE. there could be a case where this gets stuck
        // where it doesn't see the next chunk for the guid
        // and never receives another one for that guid. Need
        // to add a timeout.
        // ignore if this is another concurrent
        // file transfer
        if gcode.guid != self.guid {
            return Ok(Some(self));
        }
        // Duplicate packet that may have slipped through.
        if gcode.chunk_id == self.id {
            return Ok(Some(self));
        }
        // If it is not the next increment delete
        if gcode.chunk_id != self.id + 1 {
            let fil = self.fil;
            fil.close();
            let partial_path = heapless::format!(64; "/usb/{}.partial", gcode.guid).unwrap();
            V::delete(&partial_path).await?;
            return Ok(None);
        }
        // it is so write the data
        if let Err(_err) = self.fil.write_all(gcode.data) {
            let fil = self.fil;
            fil.close();
            let partial_path = heapless::format!(64; "/usb/{}.partial", gcode.guid).unwrap();
            V::delete(&partial_path).await?;
            return Ok(None);
        };
        self.id += 1;
        if gcode.last_chunk {
            let fil = self.fil;
            fil.close();
            let partial_path = heapless::format!(64; "/usb/{}.partial", gcode.guid).unwrap();
            let final_path = heapless::format!(64; "/usb/{}.gcode", gcode.guid).unwrap();
            V::rename(partial_path.as_str(), final_path.as_str()).await?;
            return Ok(None);
        }
        Ok(Some(self))
    }
}
