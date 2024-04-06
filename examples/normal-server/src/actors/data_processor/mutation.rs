use crate::entities::{data, data_transaction, prelude::*};
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait};

pub struct Mutation(pub DatabaseConnection);

impl Mutation {
    pub async fn insert_data_transaction(
        &self,
        data_transaction: data_transaction::ActiveModel,
    ) -> Result<data_transaction::Model, sea_orm::prelude::DbErr> {
        data_transaction.insert(&self.0).await
    }

    pub async fn bulk_insert_data<I: IntoIterator<Item = data::ActiveModel>>(
        &self,
        models: I,
    ) -> Result<sea_orm::InsertResult<data::ActiveModel>, sea_orm::prelude::DbErr> {
        Data::insert_many(models).exec(&self.0).await
    }
}
