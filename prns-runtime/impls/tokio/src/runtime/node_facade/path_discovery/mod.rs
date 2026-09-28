use prns_core::entropy::EntropySource;

use crate::engine::{
    PathFound, PathRequestId, PrnsCommand, RequestPath, Settlement, PATH_REQUEST_ID_LEN,
};
use crate::runtime::OsEntropySource;
use crate::wire::DestinationHash;

use super::{PrnsNodeHandle, RequestPathError};

impl PrnsNodeHandle {
    pub async fn request_path(
        &self,
        destination: DestinationHash,
    ) -> Result<PathFound, RequestPathError> {
        self.request_path_with_source(destination, &mut OsEntropySource)
            .await
    }

    async fn request_path_with_source<S: EntropySource>(
        &self,
        destination: DestinationHash,
        source: &mut S,
    ) -> Result<PathFound, RequestPathError> {
        let mut request_id = [0; PATH_REQUEST_ID_LEN];
        source
            .try_fill_entropy(&mut request_id)
            .map_err(|_| RequestPathError::EntropyUnavailable)?;
        let timing = self.path_command_timing().await;
        match self
            .settle_with_timing(
                PrnsCommand::RequestPath(RequestPath {
                    destination,
                    id: PathRequestId::new(request_id),
                }),
                timing,
            )
            .await
        {
            Some(Settlement::RequestPath(result)) => result.map_err(RequestPathError::Failed),
            Some(_) | None => Err(RequestPathError::NodeStopped),
        }
    }
}

#[cfg(test)]
mod tests;
