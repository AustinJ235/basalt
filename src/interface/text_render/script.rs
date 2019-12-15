use allsorts::tag;

#[derive(Clone,Debug,PartialEq)]
pub enum BstTextScript {
	Default,
}

impl BstTextScript {
	pub(crate) fn tag(&self) -> u32 {
		match self {
			&BstTextScript::Default => tag::from_string("DFLT").unwrap(),
		}
	}
}

#[derive(Clone,Debug,PartialEq)]
pub enum BstTextLang {
	Default,
}

impl BstTextLang {
	pub(crate) fn tag(&self) -> u32 {
		match self {
			&BstTextLang::Default => tag::from_string("dflt").unwrap(),
		}
	}
}
