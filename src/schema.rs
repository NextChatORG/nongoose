mod before;
mod data;
pub mod types;

pub use before::SchemaBefore;
pub use data::SchemaData;
use mongodb::{
  bson::{bson, doc, Bson, Document},
  options::ReplaceOptions,
  sync::Database,
};
#[cfg(feature = "async")]
use tokio::task::spawn_blocking;

use crate::errors::Result;

use self::types::SchemaRelationType;

/// Schema
///
/// This trait is defined through the [`async-trait`](https://crates.io/crates/async-trait) macro.
#[cfg_attr(feature = "async", async_trait::async_trait)]
pub trait Schema: SchemaBefore {
  type Id: Into<Bson> + Clone + Send;

  #[doc(hidden)]
  fn __get_database(database: Option<Database>) -> &'static Database;

  #[doc(hidden)]
  fn __get_collection_name() -> String;

  #[doc(hidden)]
  fn __get_id(&self) -> Self::Id;

  #[doc(hidden)]
  fn __get_id_query(&self) -> Document {
    doc! { "_id": self.__get_id().into() }
  }

  #[doc(hidden)]
  fn __to_document(&self) -> Result<Document> {
    let bson: Bson = self.into();

    match bson.as_document() {
      Some(doc) => Ok(doc.clone()),
      None => unreachable!(),
    }
  }

  #[doc(hidden)]
  fn __check_unique_fields(&self) -> Result<()>;

  #[doc(hidden)]
  fn __relations() -> Vec<types::SchemaRelation>;

  #[doc(hidden)]
  fn __get_relations(&self) -> Option<Vec<types::SchemaRelation>>;

  #[doc(hidden)]
  fn __set_relations(&mut self, field: &str, new_value: Bson) -> Result<()>;

  #[doc(hidden)]
  fn __populate_sync(mut self, field: &str) -> Result<Self> {
    let database = Self::__get_database(None);

    if let Some(relations) = self.__get_relations() {
      for relation in relations.iter() {
        if relation.field_ident == field {
          let collection_name = &relation.schema_name;

          if relation.relation_type == SchemaRelationType::OneToOne
            || relation.relation_type == SchemaRelationType::ManyToOne
          {
            if let Some(data) = database
              .collection::<Document>(collection_name.as_str())
              .find_one(Some(doc! { "_id": relation.field_value.clone() }), None)?
            {
              self.__set_relations(field, Bson::Document(data))?;
            }
          } else if relation.relation_type == SchemaRelationType::OneToMany {
            if let Some(schema) = crate::nongoose::globals::get_schema(collection_name) {
              for schema_relation in schema.get_relations().iter() {
                if schema_relation.relation_type != SchemaRelationType::ManyToOne {
                  continue;
                }

                if schema_relation.schema_name == Self::__get_collection_name() {
                  let documents: Vec<mongodb::error::Result<Document>> = database
                    .collection::<Document>(collection_name.as_str())
                    .find(
                      Some(doc! { schema_relation.field_id(): self.__get_id().into() }),
                      None,
                    )?
                    .collect();

                  let mut data = Vec::new();
                  for doc in documents {
                    let doc = doc?;
                    data.push(doc);
                  }

                  self.__set_relations(field, bson!(data))?;
                  break;
                }
              }
            }
          }
        }
      }
    }

    Ok(self)
  }

  #[cfg(not(feature = "async"))]
  fn populate(self, field: &str) -> Result<Self> {
    self.__populate_sync(field)
  }

  #[cfg(feature = "async")]
  async fn populate(mut self, field: &'static str) -> Result<Self>
  where
    Self: 'static,
  {
    spawn_blocking(move || self.__populate_sync(field)).await?
  }

  #[cfg(not(feature = "async"))]
  fn save(mut self) -> Result<Self> {
    let db = Self::__get_database(None);
    let collection = db.collection::<Document>(Self::__get_collection_name().as_str());

    self.__check_unique_fields()?;

    if collection
      .find_one(Some(self.__get_id_query()), None)?
      .is_some()
    {
      self.before_update(db)?;

      let id_query = self.__get_id_query();
      let document = self.__to_document()?;

      collection.replace_one(
        id_query,
        document,
        Some(ReplaceOptions::builder().upsert(true).build()),
      )?;
    } else {
      self.before_create(db)?;

      let document = self.__to_document()?;
      collection.insert_one(document, None)?;
    }

    Ok(self)
  }

  #[cfg(feature = "async")]
  async fn save(mut self) -> Result<Self>
  where
    Self: 'static,
  {
    let db = Self::__get_database(None);
    let collection = db.collection::<Document>(Self::__get_collection_name().as_str());

    self.__check_unique_fields()?;

    if collection
      .find_one(Some(self.__get_id_query().clone()), None)?
      .is_some()
    {
      self.before_update(db).await?;

      let id_query = self.__get_id_query();
      let document = self.__to_document()?;

      spawn_blocking(move || {
        collection.replace_one(
          id_query,
          document,
          Some(ReplaceOptions::builder().upsert(true).build()),
        )
      })
      .await??;
    } else {
      self.before_create(db).await?;

      let document = self.__to_document()?;
      spawn_blocking(move || collection.insert_one(document, None)).await??;
    }

    Ok(self)
  }
}
