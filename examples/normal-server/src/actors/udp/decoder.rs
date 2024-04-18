use normal_data::Data;

pub(super) struct DataDecoder;

impl tokio_util::codec::Decoder for DataDecoder {
    type Item = Data;

    type Error = anyhow::Error;

    fn decode(
        &mut self,
        src: &mut tokio_util::bytes::BytesMut,
    ) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() == 0 {
            return Ok(None);
        }

        let buf: &[u8] = src.as_ref();
        let data: Data = serde_json::from_slice(buf)?;
        src.clear();

        Ok(Some(data))
    }
}
