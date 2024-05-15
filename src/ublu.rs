#![allow(clippy::needless_range_loop)]
#![allow(unused_assignments)]
#![allow(unused_variables)]
#![allow(dead_code)]
use ark_ec::pairing::Pairing;
use ark_ff::UniformRand;
use ark_std::Zero;
use rand::RngCore;
use stirling_numbers::stirling2_table;

use crate::{
    ch20::{AlgInst, AlgWit, CH20Proof, CH20CRS},
    commitment::{Comm, PedersenParams},
    consistency,
    elgamal::{Cipher, ElgamalParams, ElgamalSk},
    languages::key_lang,
    sigma::SigmaProof,
    utils::{binomial, field_pow},
};

#[allow(dead_code)]
pub struct Ublu<P: Pairing, RNG: RngCore> {
    rng: RNG,
    lambda: usize,
    d: usize,
    g: P::G1,
    com_h: P::G1,
    w: Vec<P::G1>,
    elgamal: ElgamalParams<P::G1>,
    pedersen: PedersenParams<P::G1>,
    ch20: CH20CRS<P>,
    stirling: Vec<u32>,
}

//TODO placeholder
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PkProof<P: Pairing> {
    proof: SigmaProof<P::G1>,
}

//TODO placeholder
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TagProof<P: Pairing> {
    _phantom: std::marker::PhantomData<P>,
}
impl<P: Pairing> From<CH20Proof<P>> for TagProof<P> {
    fn from(_value: CH20Proof<P>) -> Self {
        // TODO
        TagProof {
            _phantom: std::marker::PhantomData,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicKey<P: Pairing> {
    h: P::G1,
    com_t: Comm<P::G1>,
    proof_pk: PkProof<P>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecretKey<P: Pairing> {
    elgamal_sk: ElgamalSk<P::G1>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Hint<P: Pairing> {
    ciphers: Vec<Cipher<P::G1>>,
    com_x: Comm<P::G1>,
    proof_c: CH20Proof<P>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Tag<P: Pairing> {
    proof: TagProof<P>,
    com: Comm<P::G1>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XC<P: Pairing> {
    h: P::G1,
    comt_t: Comm<P::G1>,
    old_com_x: Comm<P::G1>,
    com_u: Comm<P::G1>,
    old_ciphers: Vec<Cipher<P::G1>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WC<P: Pairing> {
    x: usize,
    r_list: Vec<P::ScalarField>,
    r_x: P::ScalarField,
    alpha: P::ScalarField,
    r_alpha: P::ScalarField,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Escrow<P: Pairing> {
    /// (E1, E2)
    escrow_enc: Cipher<P::G1>,
    /// {A_i,D_i}
    blinded_ciphers: Vec<Cipher<P::G1>>,
    com_x: Comm<P::G1>,
    com_alpha: Comm<P::G1>,
    com_beta: Comm<P::G1>,
    proof_c: CH20Proof<P>,
    proof_e: CH20Proof<P>,
}

impl<P: Pairing, RNG: RngCore> Ublu<P, RNG> {
    pub fn setup(lambda: usize, d: usize, mut rng: RNG) -> Self {
        let g = P::G1::rand(&mut rng);
        let com_h = P::G1::rand(&mut rng);
        let mut w: Vec<P::G1> = Vec::with_capacity(d);
        for _i in 0..d {
            w.push(P::G1::rand(&mut rng));
        }
        let ch20: CH20CRS<P> = CH20CRS::setup(&mut rng);
        // Compute Stirling table
        let stirling: Vec<u32> = stirling2_table(d).last().unwrap().to_owned();
        // let stirling = stirling_tab.iter().map(|x| *x as usize).collect();
        Ublu {
            lambda,
            d,
            g,
            w,
            elgamal: ElgamalParams { g },
            pedersen: PedersenParams { g, h: com_h },
            ch20,
            com_h,
            rng,
            stirling,
        }
    }

    pub fn key_gen(&mut self, t: u32) -> (PublicKey<P>, SecretKey<P>, Hint<P>) {
        let rng = &mut self.rng;
        let (sk, pk) = self.elgamal.key_gen(rng);
        let r_t = P::ScalarField::rand(rng);
        let mut r_vec = Vec::with_capacity(t as usize);
        let mut cipher_vec = Vec::with_capacity(t as usize);
        for i in 1..=self.d {
            let base = -(t as i32);
            let cur_msg = base.pow(i as u32);
            let cur_r = P::ScalarField::rand(rng);
            r_vec.push(cur_r);
            cipher_vec.push(self.elgamal.encrypt_raw(&pk, cur_msg, cur_r));
        }
        let com_t = self.pedersen.commit_raw(&P::ScalarField::from(t), &r_t);
        let com_x0 = self
            .pedersen
            .commit_raw(&P::ScalarField::from(0_u32), &P::ScalarField::zero());
        let com_u0 = self
            .pedersen
            .commit_raw(&P::ScalarField::from(0_u32), &P::ScalarField::zero());

        let proof_pk = {
            let lang = key_lang(self.g, self.com_h);
            let inst = AlgInst(vec![pk.h, cipher_vec[1].b, com_t.com.value]); //B01
            let wit = AlgWit(vec![sk.sk, P::ScalarField::from(t), r_vec[1], r_t]);
            assert!(lang.contains(&inst, &wit));
            let proof = SigmaProof::prove(&lang, &inst, &wit);
            PkProof { proof }
        };

        let proof_c = {
            let x: P::ScalarField = From::from(0u64);
            let r_x: P::ScalarField = From::from(0u64);
            let alpha: P::ScalarField = From::from(0u64);
            let r_alpha: P::ScalarField = From::from(0u64);
            let rs: Vec<P::ScalarField> = (0..self.d).map(|_i| From::from(0u64)).collect();

            let hs: Vec<P::G1> = [self.com_h, pk.h]
                .into_iter()
                .chain(self.w.clone())
                .collect();

            let wit = consistency::consistency_form_wit(
                self.d,
                From::from(t),
                r_t,
                x,
                r_x,
                alpha,
                r_alpha,
                rs,
            );
            let inst = consistency::consistency_wit_to_inst(self.g, &hs, self.d, &wit);

            let lang_core = consistency::consistency_core_lang(self.g, &hs, self.d);

            let inst_core = consistency::consistency_inst_to_core(self.d, &inst);
            let wit_core = consistency::consistency_wit_to_core(&wit);

            let proof: CH20Proof<P> =
                CH20Proof::prove(&self.ch20, &lang_core, &inst_core, &wit_core);

            proof
        };

        (
            PublicKey {
                h: pk.h,
                com_t: com_t.com,
                proof_pk,
            },
            SecretKey { elgamal_sk: sk },
            Hint {
                ciphers: cipher_vec,
                com_x: com_x0.com,
                proof_c,
            },
        )
    }

    pub fn update(
        &mut self,
        pk: &PublicKey<P>,
        hint: &Hint<P>,
        _old_tag: Option<&Tag<P>>,
        x: usize,
    ) -> (Hint<P>, Tag<P>) {
        let rnd = P::ScalarField::rand(&mut self.rng);
        let r_x = P::ScalarField::rand(&mut self.rng);
        let new_hint = self.update_hint(pk, hint, x, &r_x);
        let _cur_com = self
            .pedersen
            .commit_raw(&P::ScalarField::from(x as u64), &rnd);
        // TODO do the proofs

        // @Misha: how does this work when there is no initial tag or tag proof? i.e. in the first update case.
        let proof_t = CH20Proof {
            a: Vec::new(),
            d: Vec::new(),
        };
        let new_tag = Tag {
            proof: proof_t.into(),
            com: new_hint.com_x.clone(),
        };
        (new_hint, new_tag)
    }

    fn update_hint(
        &mut self,
        pk: &PublicKey<P>,
        old_hint: &Hint<P>,
        x: usize,
        r_x: &P::ScalarField,
    ) -> Hint<P> {
        let mut r_i_list = Vec::with_capacity(self.d);
        for _i in 0..self.d {
            r_i_list.push(P::ScalarField::rand(&mut self.rng));
        }
        let x_field = P::ScalarField::from(x as u64);
        let mut new_com = self.pedersen.commit_raw(&x_field, r_x).com;
        new_com = new_com + old_hint.com_x.clone();
        // TODO can probably be optimized a bit by modifying cipherts in place
        let new_ciphers = self.update_powers2(old_hint.ciphers.clone(), r_i_list.clone(), x, pk.h);
        let com_u = self
            .pedersen
            .commit_raw(&P::ScalarField::from(0_u32), &P::ScalarField::zero());
        let _xc: XC<P> = XC {
            h: pk.h,
            comt_t: pk.com_t.clone(),
            old_com_x: old_hint.com_x.clone(),
            com_u: com_u.com,
            old_ciphers: old_hint.ciphers.clone(),
        };
        let _wc: WC<P> = WC {
            x,
            r_list: r_i_list,
            r_x: *r_x,
            alpha: P::ScalarField::zero(),
            r_alpha: P::ScalarField::zero(),
        };
        // TODO do update proof
        let pi_c: CH20Proof<P> = CH20Proof {
            a: Vec::new(),
            d: Vec::new(),
        };

        Hint {
            ciphers: new_ciphers,
            com_x: new_com,
            proof_c: pi_c,
        }
    }

    fn update_powers(
        &mut self,
        old_ciphers: Vec<Cipher<P::G1>>,
        r_i_list: Vec<P::ScalarField>,
        x: P::ScalarField,
        pk_h: P::G1,
    ) -> Vec<Cipher<P::G1>> {
        let v_coeff =
            |i: usize, j: usize| field_pow(x, i - j) * P::ScalarField::from(binomial(i, j) as u64);
        let mut new_ciphers: Vec<Cipher<P::G1>> = vec![];

        for i in 0..old_ciphers.len() {
            // TODO fails with negative subtraction because the j's should only iterate up to i, but goes all the way to d
            let a = old_ciphers
                .iter()
                .enumerate()
                .map(|(j, c_j)| c_j.a * v_coeff(i + 1, j + 1))
                .reduce(|x, y| x + y)
                .unwrap()
                + self.g * r_i_list[i];
            let b = old_ciphers
                .iter()
                .enumerate()
                .map(|(j, c_j)| c_j.b * v_coeff(i + 1, j + 1))
                .reduce(|x, y| x + y)
                .unwrap()
                + self.g * field_pow(x, i + 1)
                + pk_h * r_i_list[i];

            new_ciphers.push(Cipher { a, b });
        }

        new_ciphers
    }

    fn update_powers2(
        &mut self,
        old_ciphers: Vec<Cipher<P::G1>>,
        r_i_list: Vec<P::ScalarField>,
        x: usize,
        pk_h: P::G1,
    ) -> Vec<Cipher<P::G1>> {
        let mut new_ciphers = Vec::with_capacity(self.d);
        for i in 1..=self.d {
            let mut cur_a_res = P::G1::zero();
            let mut cur_b_res = P::G1::zero();
            for j in 1..=i {
                let cur_v_val = P::ScalarField::from(Self::v_func(x, i, j) as u64);
                cur_a_res += old_ciphers.get(j - 1).unwrap().a * cur_v_val;
                cur_b_res += old_ciphers.get(j - 1).unwrap().b * cur_v_val;
            }
            cur_a_res += self.g * r_i_list.get(i - 1).unwrap();
            cur_b_res += pk_h * r_i_list.get(i - 1).unwrap();
            new_ciphers.push(Cipher {
                a: cur_a_res,
                b: cur_b_res,
            });
        }
        new_ciphers
    }

    fn v_func(x: usize, i: usize, j: usize) -> usize {
        let bin = binomial(i, j);
        bin * x.pow((i - j) as u32)
    }

    fn blind_powers(
        &self,
        old_ciphers: &[Cipher<P::G1>],
        alpha: P::ScalarField,
    ) -> Vec<Cipher<P::G1>> {
        let mut new_ciphers: Vec<_> = old_ciphers.to_vec();
        for i in 0..new_ciphers.len() {
            new_ciphers[i].b += self.w[i] * alpha;
        }
        new_ciphers
    }

    pub fn escrow(&mut self, pk: &PublicKey<P>, hint: &Hint<P>) -> Escrow<P> {
        let alpha = P::ScalarField::rand(&mut self.rng);
        let r_alpha = P::ScalarField::rand(&mut self.rng);
        let beta = P::ScalarField::rand(&mut self.rng);
        let r_beta = P::ScalarField::rand(&mut self.rng);

        let com_alpha = self.pedersen.commit_raw(&alpha, &r_alpha).com;
        let com_beta = self.pedersen.commit_raw(&beta, &r_beta).com;

        let blinded_ciphers = self.blind_powers(&hint.ciphers, alpha);
        let escrow_enc = self.evaluate(&hint.ciphers, beta);

        // TODO placeholder for test
        let proof_c = CH20Proof {
            a: Vec::new(),
            d: Vec::new(),
        };
        let proof_e = CH20Proof {
            a: Vec::new(),
            d: Vec::new(),
        };

        Escrow {
            escrow_enc,
            blinded_ciphers,
            com_x: hint.com_x.clone(),
            com_alpha,
            com_beta,
            proof_c,
            proof_e,
        }
    }

    pub fn decrypt(&self, sk: &SecretKey<P>, escrow: &Escrow<P>) -> bool {
        let e_1 = escrow.escrow_enc.a;
        let e_2 = escrow.escrow_enc.b;
        let m = e_2 - e_1 * sk.elgamal_sk.sk;
        m == self.g
    }

    fn evaluate(&self, old_ciphers: &[Cipher<P::G1>], beta: P::ScalarField) -> Cipher<P::G1> {
        let mut e_1 = P::G1::zero();
        let mut e_2 = P::G1::zero();
        for i in 1..=self.d {
            e_1 += old_ciphers[i - 1].a * P::ScalarField::from(self.stirling[i - 1]);
            e_1 += old_ciphers[i - 1].b * P::ScalarField::from(self.stirling[i - 1]);
        }
        e_1 *= beta;
        e_2 *= beta;
        Cipher { a: e_1, b: e_2 }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::Ublu;
    use aes_prng::AesRng;
    use ark_bls12_381::Bls12_381;
    use rand::SeedableRng;

    #[test]
    fn sunshine() {
        let lambda = 40;
        let d = 10;
        let t = 5;
        let x = 4;
        let rng = AesRng::seed_from_u64(1);
        let mut ublu: Ublu<Bls12_381, AesRng> = Ublu::setup(lambda, d, rng);
        let (pk, sk, hint_0) = ublu.key_gen(t);
        let (hint_1, tag_1) = ublu.update(&pk, &hint_0, None, x);
        let escrow_1 = ublu.escrow(&pk, &hint_1);
        // We only added 4, and the threshold is 5, so we should fail
        assert!(!ublu.decrypt(&sk, &escrow_1));
        // We add 2 more, so now we should succeed
        let (hint_2, tag_2) = ublu.update(&pk, &hint_0, None, x);
        let escrow_2 = ublu.escrow(&pk, &hint_2);
        assert!(ublu.decrypt(&sk, &escrow_2));
    }
}
